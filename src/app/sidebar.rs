use chrono::{DateTime, Datelike, Days, Local, NaiveDate, Utc};
use gpui::{KeyBinding, actions};

use super::*;
use crate::ui::shortcut::ShortcutHint;
use waku_client::friends::TransferStatus;

actions!(waku_sidebar, [CancelSessionRename]);

pub(super) const SESSION_RENAME_PARENT_CONTEXT: &str = "SessionRename";
const SESSION_RENAME_FIELD_CONTEXT: &str = "SessionRename > TextInput";

/// Keep Escape inside the focused inline editor so it cancels the rename,
/// rather than falling through to the window-wide Stop action.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        CancelSessionRename,
        Some(SESSION_RENAME_FIELD_CONTEXT),
    )]);
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum SessionDateGroup {
    Today,
    Yesterday,
    ThisWeek,
    ThisMonth,
    ThisYear,
    More,
}

impl SessionDateGroup {
    const ALL: [Self; 6] = [
        Self::Today,
        Self::Yesterday,
        Self::ThisWeek,
        Self::ThisMonth,
        Self::ThisYear,
        Self::More,
    ];

    fn index(self) -> usize {
        match self {
            Self::Today => 0,
            Self::Yesterday => 1,
            Self::ThisWeek => 2,
            Self::ThisMonth => 3,
            Self::ThisYear => 4,
            Self::More => 5,
        }
    }

    fn label(self) -> String {
        match self {
            Self::Today => tr!("sidebar.today"),
            Self::Yesterday => tr!("sidebar.yesterday"),
            Self::ThisWeek => tr!("sidebar.this_week"),
            Self::ThisMonth => tr!("sidebar.this_month"),
            Self::ThisYear => tr!("sidebar.this_year"),
            Self::More => tr!("sidebar.more"),
        }
    }
}

/// Stable identity for a collapsible sidebar section. Keeping both variants in
/// one set preserves each view's disclosure state when the user switches
/// between Project and Date grouping.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum SidebarGroup {
    /// Pinned tasks, always the first section regardless of grouping.
    Pinned,
    /// Every terminal — session-scoped and global — sitting between the
    /// search field and the session history. Starts collapsed; pinned
    /// terminals keep a row while it is.
    Terminals,
    Date(SessionDateGroup),
    Project(Uuid),
    Projectless,
}

impl SidebarGroup {
    fn element_key(self) -> SharedString {
        match self {
            Self::Pinned => "pinned".into(),
            Self::Terminals => "terminals".into(),
            Self::Date(group) => format!("date-{}", group.index()).into(),
            Self::Project(project_id) => format!("project-{project_id}").into(),
            Self::Projectless => "projectless".into(),
        }
    }

    fn mix_fingerprint(self, fingerprint: u64) -> u64 {
        match self {
            Self::Pinned => mix(fingerprint, 0x300),
            Self::Terminals => mix(fingerprint, 0x400),
            Self::Date(group) => mix(fingerprint, group.index() as u64 + 1),
            Self::Project(project_id) => mix_uuid(mix(fingerprint, 0x100), project_id),
            Self::Projectless => mix(fingerprint, 0x200),
        }
    }
}

fn sidebar_grouping_label(grouping: SidebarGrouping) -> String {
    match grouping {
        SidebarGrouping::Project => tr!("sidebar.grouping_project"),
        SidebarGrouping::Date => tr!("sidebar.grouping_date"),
    }
}

fn sidebar_ordering_label(ordering: SidebarOrdering) -> String {
    match ordering {
        SidebarOrdering::LastUpdated => tr!("sidebar.ordering_last_updated"),
        SidebarOrdering::LastCreated => tr!("sidebar.ordering_last_created"),
    }
}

fn session_date_group(timestamp: u64, today: NaiveDate) -> SessionDateGroup {
    let session_date = i64::try_from(timestamp)
        .ok()
        .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        .map(|timestamp| timestamp.with_timezone(&Local).date_naive())
        .unwrap_or(today);
    session_date_group_for_dates(session_date, today)
}

fn session_date_group_for_dates(session_date: NaiveDate, today: NaiveDate) -> SessionDateGroup {
    if session_date >= today {
        return SessionDateGroup::Today;
    }

    if today.pred_opt() == Some(session_date) {
        return SessionDateGroup::Yesterday;
    }

    let week_start = today
        .checked_sub_days(Days::new(today.weekday().num_days_from_monday().into()))
        .unwrap_or(today);
    if session_date >= week_start {
        return SessionDateGroup::ThisWeek;
    }

    if session_date.year() == today.year() && session_date.month() == today.month() {
        return SessionDateGroup::ThisMonth;
    }

    if session_date.year() == today.year() {
        return SessionDateGroup::ThisYear;
    }

    SessionDateGroup::More
}

fn session_group_header(theme: &Theme, height: f32) -> Div {
    div()
        .h(px(height))
        .px(px(8.0))
        .flex()
        .items_center()
        .text_size(sp(13.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.text_secondary)
}

fn append_sidebar_group_rows(
    rows: &mut Vec<SidebarRow>,
    group: SidebarGroup,
    sessions: &[Uuid],
    collapsed: bool,
    show_more: bool,
) {
    if sessions.is_empty() && !show_more {
        return;
    }

    rows.push(SidebarRow::Header(group));
    if !collapsed {
        rows.extend(sessions.iter().copied().map(SidebarRow::Session));
        if show_more {
            rows.push(SidebarRow::ShowMore(group));
        }
    }
    rows.push(SidebarRow::GroupSpacer);
}

fn updater_button_available_content(
    foreground: Hsla,
    label: SharedString,
    label_reveal: f32,
) -> Div {
    div()
        .relative()
        .size_full()
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .opacity(1.0 - label_reveal)
                .child(icon("icons/download.svg", 12.0, foreground)),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .whitespace_nowrap()
                .opacity(label_reveal)
                .child(label),
        )
}

/// Height of a session card plus the separation reserved beneath it in the
/// virtualized sidebar list. Keep the gap inside the list row so measured and
/// estimated heights stay identical for off-screen sessions.
const SIDEBAR_SESSION_CARD_HEIGHT: f32 = 51.0;
const SIDEBAR_SESSION_ROW_GAP: f32 = 1.0;
const SIDEBAR_SESSION_ROW_HEIGHT: f32 = SIDEBAR_SESSION_CARD_HEIGHT + SIDEBAR_SESSION_ROW_GAP;
const SIDEBAR_ACTION_ROW_HEIGHT: f32 = 30.0;
/// Separation above each action button in the sidebar stack. Kept inside the
/// list row — like the session row gap — so measured and estimated heights
/// stay identical.
const SIDEBAR_ACTION_ROW_GAP: f32 = 2.0;
const SIDEBAR_GROUP_HEADER_HEIGHT: f32 = 28.0;
/// The session column's top bar. The empty-state hero drops by this much so
/// it sits clear of the header instead of optically centering under it.
const HEADER_HEIGHT: f32 = 48.0;
const SIDEBAR_GROUP_HEADER_BOTTOM_GAP: f32 = 2.0;
const SIDEBAR_SHOW_MORE_ROW_HEIGHT: f32 = 30.0;
/// The spacer a project group carries between its rows and the next group.
const SIDEBAR_GROUP_SPACER_HEIGHT: f32 = 10.0;
const SIDEBAR_GROUP_GUIDE_X: f32 = 15.0;
const SIDEBAR_GROUP_CHILD_PADDING: f32 = 28.0;
/// Chats shown under a project group before the rest fold behind "Show more".
const SIDEBAR_PROJECT_DEFAULT_VISIBLE: usize = 16;
const SIDEBAR_PROJECT_REVEAL_BATCH: usize = 30;
/// How long the primary modifier must stay down before the sidebar reveals
/// its ⌘1–⌘9 chips — long enough that quicker chords never flash them.
pub(super) const SIDEBAR_SHORTCUT_HOLD_DELAY: Duration = Duration::from_millis(400);
/// Number of sidebar tasks reachable by the ⌘1–⌘9 row shortcuts.
const SIDEBAR_SHORTCUT_TARGET_COUNT: usize = 9;
/// Width of the gradient that dissolves row content ahead of a ⌘n chip.
const SIDEBAR_SHORTCUT_CHIP_FADE_WIDTH: f32 = 28.0;
/// Git status drifts without any session-set change, so checkout-status scans
/// rerun on this cadence in addition to path-set fingerprint changes.
const SIDEBAR_CHECKOUT_STATUS_RESCAN: Duration = Duration::from_secs(10);

/// The session row's trailing time: how long ago the agent last replied,
/// shown through a live turn too. A session that has never replied shows
/// nothing.
pub(super) fn session_time_label(session: &AgentSession, now: u64) -> Option<String> {
    if session.status == SessionStatus::Background {
        return Some(tr!("sidebar.status_background"));
    }
    session
        .last_reply_at
        .map(|last_reply_at| format_time_ago(now.saturating_sub(last_reply_at)))
}

/// Recency for sidebar ordering and date groups. A submitted turn promotes the
/// task immediately, while metadata edits such as a rename do not; a task with
/// no turns stays anchored to when it was created.
pub(super) fn sidebar_session_timestamp(session: &AgentSession) -> u64 {
    session.last_reply_at.unwrap_or(session.created_at)
}

/// The ordering preference's sort key. Date groups bucket by
/// `sidebar_session_timestamp` instead; see `date_sidebar_groups`.
fn sidebar_ordering_timestamp(session: &AgentSession, ordering: SidebarOrdering) -> u64 {
    match ordering {
        SidebarOrdering::LastUpdated => sidebar_session_timestamp(session),
        SidebarOrdering::LastCreated => session.created_at,
    }
}

fn sort_sidebar_sessions(sessions: &mut Vec<&AgentSession>, ordering: SidebarOrdering) {
    sessions
        .sort_by_key(|session| std::cmp::Reverse(sidebar_ordering_timestamp(session, ordering)));
}

/// Date buckets always follow last-updated recency, independent of the
/// ordering preference: a task created last week but replied to today still
/// lands under Today, sorted within the group by the ordering's key.
fn date_sidebar_groups(sessions: &[&AgentSession], today: NaiveDate) -> [Vec<Uuid>; 6] {
    let mut grouped_sessions: [Vec<Uuid>; 6] = std::array::from_fn(|_| Vec::new());
    for &session in sessions {
        grouped_sessions[session_date_group(sidebar_session_timestamp(session), today).index()]
            .push(session.id);
    }
    grouped_sessions
}

fn project_sidebar_groups(
    sessions: &[&AgentSession],
    projectless_project_ids: &HashSet<Uuid>,
) -> Vec<(SidebarGroup, Vec<Uuid>)> {
    let mut groups: Vec<(SidebarGroup, Vec<Uuid>)> = Vec::new();
    let mut indexes = HashMap::new();
    let mut projectless_sessions = Vec::new();
    for session in sessions {
        if projectless_project_ids.contains(&session.project_id) {
            projectless_sessions.push(session.id);
            continue;
        }
        let index = *indexes.entry(session.project_id).or_insert_with(|| {
            let index = groups.len();
            groups.push((SidebarGroup::Project(session.project_id), Vec::new()));
            index
        });
        groups[index].1.push(session.id);
    }
    if !projectless_sessions.is_empty() {
        groups.push((SidebarGroup::Projectless, projectless_sessions));
    }
    groups
}

fn visible_project_sessions(
    sessions: &[Uuid],
    revealed_extra_sessions: usize,
) -> (Vec<Uuid>, bool) {
    let limit = SIDEBAR_PROJECT_DEFAULT_VISIBLE.saturating_add(revealed_extra_sessions);
    let visible = sessions.iter().take(limit).copied().collect();
    (visible, sessions.len() > limit)
}

fn sidebar_project_is_projectless(project: &Project, projectless_root: Option<&Path>) -> bool {
    projectless_root.is_some_and(|root| project.path.starts_with(root))
}

fn persisted_sidebar_branch_label(workspace: &SessionWorkspace) -> Option<&str> {
    match workspace {
        SessionWorkspace::Local => None,
        SessionWorkspace::NewWorktree { base_branch } => base_branch.as_deref(),
        SessionWorkspace::Worktree { name, .. } => Some(name.as_str()),
    }
    .filter(|branch| !branch.is_empty())
}

pub(super) fn mix_str(hash: u64, value: &str) -> u64 {
    value
        .bytes()
        .fold(hash, |hash, byte| mix(hash, byte as u64))
}

fn sidebar_status_rank(status: SessionStatus) -> u64 {
    match status {
        SessionStatus::Idle => 0,
        SessionStatus::Connecting => 1,
        SessionStatus::Working => 2,
        SessionStatus::Background => 3,
        SessionStatus::Waiting => 4,
        SessionStatus::Failed => 5,
    }
}

/// The state a sidebar pull-request badge reports. `Draft` is its own shape
/// because the glyph carries the state — color only reinforces it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SidebarPullRequestState {
    Open,
    Draft,
    Merged,
    Closed,
}

pub(super) struct SidebarPullRequestBadge {
    pub state: SidebarPullRequestState,
    pub number: u64,
    pub others: usize,
    pub title: String,
    pub check_status: Option<waku_client::PullRequestCheckStatus>,
    pub review_decision: Option<waku_client::PullRequestReviewDecision>,
}

pub(super) fn pull_request_class(
    entry: &waku_client::PullRequestSummary,
) -> SidebarPullRequestState {
    match (entry.state, entry.is_draft) {
        (waku_client::PullRequestState::Open, true) => SidebarPullRequestState::Draft,
        (waku_client::PullRequestState::Open, false) => SidebarPullRequestState::Open,
        (waku_client::PullRequestState::Merged, _) => SidebarPullRequestState::Merged,
        (waku_client::PullRequestState::Closed, _) => SidebarPullRequestState::Closed,
    }
}

/// The span in which a pull request can be this session's own work: from the
/// first turn's start to the newest turn's end. `None` on the upper bound
/// means the window is still open — a turn is in flight or none has settled
/// — so a pull request created right now is still attributable. A session can
/// only produce pull requests while it is working; anything created before
/// its first turn or after its last one ended belongs to somebody else.
pub(super) fn session_pull_request_window(session: &AgentSession) -> (u64, Option<u64>) {
    let since = session
        .turns
        .first()
        .map(|turn| turn.started_at)
        .unwrap_or(session.created_at);
    let until = session.turns.last().and_then(|turn| turn.completed_at);
    (since, until)
}

/// The pull requests attributable to a session's activity window — the set
/// the sidebar badge collapses and the header chip menus from.
pub(super) fn session_pull_requests_in_window<'a>(
    entries: &'a [waku_client::PullRequestSummary],
    window: (u64, Option<u64>),
) -> Vec<&'a waku_client::PullRequestSummary> {
    let (since, until) = window;
    entries
        .iter()
        .filter(|entry| {
            entry.created_at.map_or(true, |created| {
                created >= since && until.map_or(true, |until| created <= until)
            })
        })
        .collect()
}

/// Collapses a session's pull requests into the one badge its row shows: a
/// state glyph for the aggregate — draft only when every one is a draft, open
/// when any is, merged when all are, closed otherwise — plus the lowest
/// number in that class, which is usually the pull request the session opened
/// first. `others` counts the remainder, so `#123 +2` reads as "two more".
/// `window` bounds attribution by when the pull request was created; a host
/// that reports no creation time keeps its row rather than losing it.
pub(super) fn sidebar_pull_request_badge(
    entries: &[waku_client::PullRequestSummary],
    window: (u64, Option<u64>),
) -> Option<SidebarPullRequestBadge> {
    let entries = session_pull_requests_in_window(entries, window);
    if entries.is_empty() {
        return None;
    }
    let all_draft = entries
        .iter()
        .all(|entry| entry.state == waku_client::PullRequestState::Open && entry.is_draft);
    let state = if all_draft {
        SidebarPullRequestState::Draft
    } else if entries
        .iter()
        .any(|entry| entry.state == waku_client::PullRequestState::Open)
    {
        SidebarPullRequestState::Open
    } else if entries
        .iter()
        .all(|entry| entry.state == waku_client::PullRequestState::Merged)
    {
        SidebarPullRequestState::Merged
    } else {
        SidebarPullRequestState::Closed
    };
    let primary = entries
        .iter()
        .filter(|entry| {
            pull_request_class(entry) == state
                || (state == SidebarPullRequestState::Open
                    && entry.state == waku_client::PullRequestState::Open)
        })
        .min_by_key(|entry| entry.number)
        .or_else(|| entries.iter().min_by_key(|entry| entry.number))?;
    Some(SidebarPullRequestBadge {
        state,
        number: primary.number,
        others: entries.len() - 1,
        title: primary.title.clone(),
        check_status: primary.check_status,
        review_decision: primary.review_decision,
    })
}

pub(super) fn sidebar_pull_request_icon(state: SidebarPullRequestState) -> &'static str {
    match state {
        SidebarPullRequestState::Open => "icons/git-pull-request-arrow.svg",
        SidebarPullRequestState::Draft => "icons/git-pull-request-draft.svg",
        SidebarPullRequestState::Merged => "icons/git-merge.svg",
        SidebarPullRequestState::Closed => "icons/git-pull-request-closed.svg",
    }
}

pub(super) fn sidebar_pull_request_color(theme: &Theme, state: SidebarPullRequestState) -> Hsla {
    match state {
        SidebarPullRequestState::Open => theme.success,
        SidebarPullRequestState::Draft => theme.text_tertiary,
        SidebarPullRequestState::Merged => theme.info,
        SidebarPullRequestState::Closed => theme.danger,
    }
}

pub(super) fn sidebar_pull_request_state_label(state: SidebarPullRequestState) -> String {
    match state {
        SidebarPullRequestState::Open => tr!("sidebar.pull_request_open"),
        SidebarPullRequestState::Draft => tr!("sidebar.pull_request_draft"),
        SidebarPullRequestState::Merged => tr!("sidebar.pull_request_merged"),
        SidebarPullRequestState::Closed => tr!("sidebar.pull_request_closed"),
    }
}

/// The checks glyph sits after the PR number; the hourglass stays static —
/// the badge already refreshes on a scan cadence, so spinning adds motion
/// without adding information.
pub(super) fn sidebar_check_status_icon(
    status: waku_client::PullRequestCheckStatus,
) -> &'static str {
    match status {
        waku_client::PullRequestCheckStatus::Passing => "icons/check.svg",
        waku_client::PullRequestCheckStatus::Pending => "icons/hourglass.svg",
        waku_client::PullRequestCheckStatus::Failing => "icons/x.svg",
    }
}

pub(super) fn sidebar_check_status_color(
    theme: &Theme,
    status: waku_client::PullRequestCheckStatus,
) -> Hsla {
    match status {
        waku_client::PullRequestCheckStatus::Passing => theme.success,
        waku_client::PullRequestCheckStatus::Pending => theme.warning,
        waku_client::PullRequestCheckStatus::Failing => theme.danger,
    }
}

pub(super) fn sidebar_check_status_label(status: waku_client::PullRequestCheckStatus) -> String {
    match status {
        waku_client::PullRequestCheckStatus::Passing => {
            tr!("sidebar.pull_request_checks_passing")
        }
        waku_client::PullRequestCheckStatus::Pending => {
            tr!("sidebar.pull_request_checks_pending")
        }
        waku_client::PullRequestCheckStatus::Failing => {
            tr!("sidebar.pull_request_checks_failing")
        }
    }
}

pub(super) fn sidebar_review_decision_icon(
    decision: waku_client::PullRequestReviewDecision,
) -> &'static str {
    match decision {
        waku_client::PullRequestReviewDecision::Approved => "icons/check.svg",
        waku_client::PullRequestReviewDecision::ChangesRequested => "icons/alert.svg",
        waku_client::PullRequestReviewDecision::ReviewRequired => "icons/eye.svg",
    }
}

pub(super) fn sidebar_review_decision_color(
    theme: &Theme,
    decision: waku_client::PullRequestReviewDecision,
) -> Hsla {
    match decision {
        waku_client::PullRequestReviewDecision::Approved => theme.success,
        waku_client::PullRequestReviewDecision::ChangesRequested => theme.warning,
        waku_client::PullRequestReviewDecision::ReviewRequired => theme.text_secondary,
    }
}

fn sidebar_review_decision_label(decision: waku_client::PullRequestReviewDecision) -> String {
    match decision {
        waku_client::PullRequestReviewDecision::Approved => {
            tr!("sidebar.pull_request_review_approved")
        }
        waku_client::PullRequestReviewDecision::ChangesRequested => {
            tr!("sidebar.pull_request_review_changes")
        }
        waku_client::PullRequestReviewDecision::ReviewRequired => {
            tr!("sidebar.pull_request_review_required")
        }
    }
}

pub(super) fn sidebar_pull_request_tooltip(badge: &SidebarPullRequestBadge) -> String {
    let state = sidebar_pull_request_state_label(badge.state);
    let mut tooltip = if badge.others == 0 {
        tr!(
            "sidebar.pull_request",
            number = badge.number,
            state = state,
            title = badge.title
        )
    } else {
        tr!(
            "sidebar.pull_request_more",
            number = badge.number,
            state = state,
            title = badge.title,
            count = badge.others
        )
    };
    if let Some(status) = badge.check_status {
        tooltip.push_str(" · ");
        tooltip.push_str(&sidebar_check_status_label(status));
    }
    if let Some(decision) = badge.review_decision {
        tooltip.push_str(" · ");
        tooltip.push_str(&sidebar_review_decision_label(decision));
    }
    tooltip
}

/// Compact "how long ago" for the sidebar: "just now", then one coarse unit —
/// "5m", "3h", "420d". Days are the largest unit so a glance still reads as a
/// count rather than a date.
pub(super) fn format_time_ago(seconds: u64) -> String {
    match seconds {
        0..=59 => tr!("sidebar.just_now"),
        60..=3_599 => tr!("sidebar.minutes_ago", count = seconds / 60),
        3_600..=86_399 => tr!("sidebar.hours_ago", count = seconds / 3_600),
        _ => tr!("sidebar.days_ago", count = seconds / 86_400),
    }
}

/// One row of the virtualized sidebar session history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SidebarRow {
    /// Opens the window-wide command palette and scrolls with history.
    Search,
    /// Opens the Projects page and scrolls with history.
    Projects,
    /// Opens the GitHub notification inbox and scrolls with history.
    Inbox,
    /// Group header; the first row also carries the sidebar actions.
    Header(SidebarGroup),
    /// A started session.
    Session(Uuid),
    /// A terminal in the Terminals group.
    Terminal(Uuid),
    /// Reveals the next batch of older sessions in a project section.
    ShowMore(SidebarGroup),
    /// Spacing between date groups.
    GroupSpacer,
}

pub(super) fn sidebar_session_row_index(rows: &[SidebarRow], session_id: Uuid) -> Option<usize> {
    rows.iter()
        .position(|row| *row == SidebarRow::Session(session_id))
}

/// The first session row at-or-below `position` that `is_available` accepts,
/// scanning downward and wrapping to the top. Non-session rows are skipped.
pub(super) fn next_sidebar_session_in_rows(
    rows: &[SidebarRow],
    position: usize,
    is_available: impl Fn(Uuid) -> bool,
) -> Option<Uuid> {
    if rows.is_empty() {
        return None;
    }
    let start = position % rows.len();
    (0..rows.len())
        .map(|offset| (start + offset) % rows.len())
        .filter_map(|index| match rows[index] {
            SidebarRow::Session(session_id) => Some(session_id),
            _ => None,
        })
        .find(|session_id| is_available(*session_id))
}

/// Sessions reachable by ⌘1–⌘9, in displayed order. Reading the row snapshot
/// — rather than re-walking sessions — means folded groups, project "show
/// more" overflow, and the pinned-first layout all apply for free.
fn sidebar_shortcut_target_ids(rows: &[SidebarRow]) -> impl Iterator<Item = Uuid> + '_ {
    rows.iter().filter_map(|row| match row {
        SidebarRow::Session(session_id) => Some(*session_id),
        _ => None,
    })
}

/// The label a row chip advertises — "⌘3" on macOS, "Ctrl+3" elsewhere.
fn sidebar_shortcut_chip_label(index: usize) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{}", index + 1)
    } else {
        format!("Ctrl+{}", index + 1)
    }
}

fn sidebar_row_height(row: SidebarRow) -> Pixels {
    px(match row {
        SidebarRow::Search | SidebarRow::Projects | SidebarRow::Inbox => {
            SIDEBAR_ACTION_ROW_HEIGHT + SIDEBAR_ACTION_ROW_GAP
        }
        SidebarRow::Header(SidebarGroup::Terminals) => {
            SIDEBAR_ACTION_ROW_HEIGHT + SIDEBAR_ACTION_ROW_GAP + SIDEBAR_GROUP_HEADER_BOTTOM_GAP
        }
        SidebarRow::Header(_) => SIDEBAR_GROUP_HEADER_HEIGHT + SIDEBAR_GROUP_HEADER_BOTTOM_GAP,
        SidebarRow::Session(_) => SIDEBAR_SESSION_ROW_HEIGHT,
        SidebarRow::Terminal(_) => terminals::SIDEBAR_TERMINAL_ROW_HEIGHT,
        SidebarRow::ShowMore(_) => SIDEBAR_SHOW_MORE_ROW_HEIGHT,
        SidebarRow::GroupSpacer => SIDEBAR_GROUP_SPACER_HEIGHT,
    })
}

fn sidebar_bottom_aligned_offset(
    rows: &[SidebarRow],
    target: usize,
    viewport_height: Pixels,
) -> ListOffset {
    let mut item_ix = target;
    let mut height = sidebar_row_height(rows[target]);
    while item_ix > 0 && height < viewport_height {
        item_ix -= 1;
        height += sidebar_row_height(rows[item_ix]);
    }
    ListOffset {
        item_ix,
        offset_in_item: (height - viewport_height).max(Pixels::ZERO),
    }
}

fn reveal_sidebar_list_row(list: &ListState, rows: &[SidebarRow], index: usize) {
    let viewport = list.viewport_bounds();
    if viewport.size.height <= Pixels::ZERO {
        return;
    }
    if let Some(item) = list.bounds_for_item(index) {
        if item.top() >= viewport.top() && item.bottom() <= viewport.bottom() {
            return;
        }
        list.scroll_to_reveal_item(index);
    } else if index <= list.logical_scroll_top().item_ix {
        list.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: Pixels::ZERO,
        });
    } else {
        // Off-screen rows have not necessarily been measured yet. Their
        // sidebar heights are fixed, so align a lower target to the viewport
        // bottom just like scrollIntoView({ block: "nearest" }).
        list.scroll_to(sidebar_bottom_aligned_offset(
            rows,
            index,
            viewport.size.height,
        ));
    }
}

impl Waku {
    pub(super) fn window_drag_region(
        &self,
        region: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        // Windows drags from the hit test, not from a mouse-move handler:
        // `DefWindowProc` moves the window once the region reports itself as
        // caption, and performs the user's configured double-click action.
        #[cfg(target_os = "windows")]
        let region = region.window_control_area(gpui::WindowControlArea::Drag);

        region
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    crate::platform::titlebar_double_click(window);
                }
            })
            .on_mouse_down_out(cx.listener(|this, _, _, _| {
                this.header_drag_armed = false;
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.header_drag_armed = true;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.header_drag_armed = false;
                }),
            )
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if this.header_drag_armed {
                    this.header_drag_armed = false;
                    crate::platform::start_window_move(window);
                }
            }))
    }
    // ── Sidebar ────────────────────────────────────────────────────────────

    fn render_fps_counter(&self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let fps = self.fps_value;
        let dot = if fps == 0 {
            theme.text_ghost
        } else if fps >= 55 {
            theme.success
        } else if fps >= 30 {
            theme.warning
        } else {
            theme.danger
        };
        div()
            .flex_none()
            .h(px(26.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(5.0))
            .text_size(sp(12.5))
            .line_height(sp(0.0))
            .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(dot))
            .child(
                div()
                    .text_color(theme.text_tertiary)
                    .font_family(crate::fonts::current(cx).code)
                    .child(SharedString::from(format!("{fps} FPS"))),
            )
    }

    fn render_sidebar_toggle(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        div()
            .id("toggle-sidebar")
            .w(px(26.0))
            .h(px(26.0))
            .flex_none()
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .tooltip(Tooltip::text_with_action(
                tr!("menu.toggle_sidebar"),
                &ToggleSidebar,
            ))
            .child(icon("icons/panel-left.svg", 14.0, theme.text_tertiary))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.set_sidebar_visible(!this.sidebar_visible, cx);
            }))
    }

    /// Mouse twin of GoToNextUnreadCompletion (⌘D / ctrl-backtick): live
    /// while an off-screen task is unread — blocked on its user, or holding an
    /// unseen finished turn. A blocked target earns a red X; anything else
    /// carries the same informational-blue dot the sidebar draws in that
    /// row's status slot.
    fn render_unseen_completion_bell(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        let selected = self.state.selected_session;
        let target = sessions::next_unread_completion(
            &self.state.sessions,
            &self.state.unseen_completions,
            &rows,
            selected,
            self.pending_session_activation
                .map(|pending| pending.session_id),
        );
        let enabled = target.is_some();
        // A blocked task outranks plain completions, so the target being one
        // is what the badge warns about.
        let blocked = target.is_some_and(|session_id| {
            self.state
                .sessions
                .iter()
                .any(|session| session.id == session_id && session.status == SessionStatus::Waiting)
        });
        div()
            .id("unseen-completion-bell")
            .w(px(26.0))
            .h(px(26.0))
            .flex_none()
            .relative()
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .tooltip(Tooltip::text_with_action(
                tr!("command_palette.go_to_next_unread_completion"),
                &GoToNextUnreadCompletion,
            ))
            .when(!enabled, |element| element.opacity(0.35))
            .when(enabled, |element| {
                element
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.go_to_next_unread_completion_action(
                            &GoToNextUnreadCompletion,
                            window,
                            cx,
                        );
                    }))
            })
            .child(icon("icons/bell.svg", 14.0, theme.text_tertiary))
            .when(enabled, |element| {
                element.child(if blocked {
                    div().absolute().top(px(2.0)).right(px(2.0)).child(icon(
                        "icons/x.svg",
                        8.0,
                        theme.danger,
                    ))
                } else {
                    div()
                        .absolute()
                        .top(px(3.0))
                        .right(px(3.0))
                        .size(px(6.0))
                        .rounded_full()
                        .bg(theme.info)
                })
            })
    }

    pub(super) fn render_history_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        enabled: bool,
        navigate_back: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let (label, action): (String, &dyn gpui::Action) = if navigate_back {
            (tr!("navigation.back"), &NavigateBack)
        } else {
            (tr!("navigation.forward"), &NavigateForward)
        };
        div()
            .id(id)
            .w(px(26.0))
            .h(px(26.0))
            .flex_none()
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .tooltip(Tooltip::text_with_action(label, action))
            .when(!enabled, |element| element.opacity(0.35))
            .when(enabled, |element| {
                element
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        if navigate_back {
                            this.navigate_back_action(&NavigateBack, window, cx);
                        } else {
                            this.navigate_forward_action(&NavigateForward, window, cx);
                        }
                    }))
            })
            .child(icon(icon_path, 14.0, theme.text_tertiary))
    }

    fn render_sidebar_titlebar(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("sidebar-titlebar")
            .h(px(48.0))
            .flex_none()
            .flex()
            .items_center()
            .children(self.render_client_window_controls(
                super::window_chrome::WindowControlSide::Left,
                window,
                cx,
            ))
            .child(
                self.window_drag_region(
                    div()
                        .id("sidebar-traffic-light-drag-region")
                        .w(px(TRAFFIC_LIGHT_CLEARANCE))
                        .h_full()
                        .flex_none(),
                    cx,
                ),
            )
            .child(self.render_sidebar_toggle(cx))
            .child(self.render_unseen_completion_bell(cx).ml(px(6.0)))
            .child(
                div()
                    .ml(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .child(self.render_history_button(
                        "navigate-back",
                        "icons/arrow-left.svg",
                        !self.session_navigation.back.is_empty(),
                        true,
                        cx,
                    ))
                    .child(self.render_history_button(
                        "navigate-forward",
                        "icons/arrow-right.svg",
                        !self.session_navigation.forward.is_empty(),
                        false,
                        cx,
                    )),
            )
            .child(self.window_drag_region(
                div().id("sidebar-titlebar-drag-region").h_full().flex_1(),
                cx,
            ))
    }

    fn render_sidebar_header_actions(&self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let menu = self.menu_handle("sidebar-options", cx);
        let menu_open = menu.is_open();
        let weak = cx.entity().downgrade();
        let grouping = self.state.sidebar_grouping;
        let ordering = self.state.sidebar_ordering;
        let options = dropdown_menu(
            div()
                .id("sidebar-options")
                .w(px(20.0))
                .h(px(20.0))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                .when(menu_open, |element| element.bg(theme.overlay_strong))
                .hover(|element| element.bg(theme.overlay))
                .active(|element| element.bg(theme.overlay_strong))
                .tooltip(Tooltip::text(tr!("sidebar.options")))
                .child(icon("icons/ellipsis.svg", 14.0, theme.text_secondary)),
            "sidebar-options-menu",
            &menu,
            MenuAlign::BelowLeft,
            move |_| {
                let new_project_weak = weak.clone();
                let grouping_weak = weak.clone();
                let ordering_weak = weak.clone();
                vec![
                    MenuItem::new(tr!("project.new_project"), move |_, cx| {
                        let _ = new_project_weak.update(cx, |this, cx| {
                            this.add_project(cx);
                        });
                    })
                    .shortcut_action(&NewProject),
                    MenuItem::Separator,
                    MenuItem::submenu_with_value(
                        tr!("sidebar.grouping"),
                        sidebar_grouping_label(grouping),
                        move |_| {
                            let project_weak = grouping_weak.clone();
                            let date_weak = grouping_weak.clone();
                            vec![
                                MenuItem::new(tr!("sidebar.grouping_project"), move |_, cx| {
                                    let _ = project_weak.update(cx, |this, cx| {
                                        this.set_sidebar_grouping(SidebarGrouping::Project, cx);
                                    });
                                })
                                .selected(grouping == SidebarGrouping::Project),
                                MenuItem::new(tr!("sidebar.grouping_date"), move |_, cx| {
                                    let _ = date_weak.update(cx, |this, cx| {
                                        this.set_sidebar_grouping(SidebarGrouping::Date, cx);
                                    });
                                })
                                .selected(grouping == SidebarGrouping::Date),
                            ]
                        },
                    ),
                    MenuItem::submenu_with_value(
                        tr!("sidebar.ordering"),
                        sidebar_ordering_label(ordering),
                        move |_| {
                            let updated_weak = ordering_weak.clone();
                            let created_weak = ordering_weak.clone();
                            vec![
                                MenuItem::new(
                                    tr!("sidebar.ordering_last_updated"),
                                    move |_, cx| {
                                        let _ = updated_weak.update(cx, |this, cx| {
                                            this.set_sidebar_ordering(
                                                SidebarOrdering::LastUpdated,
                                                cx,
                                            );
                                        });
                                    },
                                )
                                .selected(ordering == SidebarOrdering::LastUpdated),
                                MenuItem::new(
                                    tr!("sidebar.ordering_last_created"),
                                    move |_, cx| {
                                        let _ = created_weak.update(cx, |this, cx| {
                                            this.set_sidebar_ordering(
                                                SidebarOrdering::LastCreated,
                                                cx,
                                            );
                                        });
                                    },
                                )
                                .selected(ordering == SidebarOrdering::LastCreated),
                            ]
                        },
                    ),
                ]
            },
        );

        div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(options)
    }

    fn render_sidebar_action_row(
        &self,
        id: &'static str,
        icon_path: &'static str,
        label: String,
        shortcut_hint: ShortcutHint,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let group_name = SharedString::from(format!("{id}-shortcut"));
        let shortcut = shortcut_hint.resolve(window, cx);
        div()
            .id(id)
            .group(group_name.clone())
            .tab_index(0)
            .w_full()
            .h(px(SIDEBAR_ACTION_ROW_HEIGHT))
            .flex_none()
            .px(px(4.0))
            .rounded(px(9.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
            .hover(|element| element.bg(theme.sidebar_item_background))
            .active(|element| element.bg(theme.overlay_strong))
            .child(
                div()
                    .size(px(20.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(icon_path, 16.0, theme.text_secondary)),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(sp(14.0))
                    .text_color(theme.text_secondary)
                    .child(label),
            )
            .when_some(shortcut, |element, shortcut| {
                element.child(
                    div()
                        .flex_none()
                        .invisible()
                        .group_hover(group_name.clone(), |element| element.visible())
                        .pr(px(4.0))
                        .text_size(sp(12.0))
                        .text_color(theme.text_tertiary)
                        .child(shortcut),
                )
            })
    }

    fn render_sidebar_new_session(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        self.render_sidebar_action_row(
            "sidebar-new-session",
            "icons/compose.svg",
            tr!("menu.new_task"),
            // ⌘N is registered to the project switcher, which propagates the
            // chord here when no draft can take it.
            ShortcutHint::action(&NewSession).shadowed_by(&SwitchProjectForward),
            window,
            cx,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.new_session_action(&NewSession, window, cx);
        }))
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                this.new_session_action(&NewSession, window, cx);
                cx.stop_propagation();
            }
        }))
    }

    fn render_sidebar_search(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let search = self
            .render_sidebar_action_row(
                "sidebar-search",
                "icons/search.svg",
                tr!("sidebar.search"),
                ShortcutHint::action(&ToggleCommandPalette),
                window,
                cx,
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_command_palette_action(&ToggleCommandPalette, window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.toggle_command_palette_action(&ToggleCommandPalette, window, cx);
                    cx.stop_propagation();
                }
            }));
        div()
            .w_full()
            .h(px(SIDEBAR_ACTION_ROW_HEIGHT + SIDEBAR_ACTION_ROW_GAP))
            .pt(px(SIDEBAR_ACTION_ROW_GAP))
            .flex_none()
            .child(search)
    }

    /// The "Projects" action row under Search — opens the page that replaces
    /// the old per-project GitHub entry.
    fn render_sidebar_projects(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let open = self.projects_page.is_some();
        let row = self
            .render_sidebar_action_row(
                "sidebar-projects",
                "icons/projects.svg",
                tr!("sidebar.projects"),
                ShortcutHint::action(&ToggleProjectsPage),
                window,
                cx,
            )
            .when(open, |element| element.bg(theme.sidebar_item_background))
            .on_click(cx.listener(|this, _, window, cx| {
                if this.projects_page.is_some() {
                    this.close_projects_page(cx);
                } else {
                    this.open_projects_page(None, window, cx);
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    if this.projects_page.is_some() {
                        this.close_projects_page(cx);
                    } else {
                        this.open_projects_page(None, window, cx);
                    }
                    cx.stop_propagation();
                }
            }));
        div()
            .w_full()
            .h(px(SIDEBAR_ACTION_ROW_HEIGHT + SIDEBAR_ACTION_ROW_GAP))
            .pt(px(SIDEBAR_ACTION_ROW_GAP))
            .flex_none()
            .child(row)
    }

    /// The notification inbox's entry — same action-row contract as
    /// Projects, plus the unread count pill GitHub's bell wears.
    fn render_sidebar_inbox(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let open = self.notifications.open;
        let unread = self.notifications.unread_count();
        let row = self
            .render_sidebar_action_row(
                "sidebar-inbox",
                "icons/bell.svg",
                tr!("sidebar.inbox"),
                ShortcutHint::action(&ToggleInboxPage),
                window,
                cx,
            )
            .when(open, |element| element.bg(theme.sidebar_item_background))
            .when(unread > 0, |element| {
                element.child(
                    div()
                        .flex_none()
                        .h(px(17.0))
                        .min_w(px(17.0))
                        .px(px(4.0))
                        .rounded_full()
                        .bg(theme.info)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(sp(10.5))
                        .text_color(theme.on_inverse)
                        .child(unread.to_string()),
                )
            })
            .on_click(cx.listener(|this, _, window, cx| {
                if this.notifications.open {
                    this.close_inbox(cx);
                } else {
                    this.open_inbox(window, cx);
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    if this.notifications.open {
                        this.close_inbox(cx);
                    } else {
                        this.open_inbox(window, cx);
                    }
                    cx.stop_propagation();
                }
            }));
        div()
            .w_full()
            .h(px(SIDEBAR_ACTION_ROW_HEIGHT + SIDEBAR_ACTION_ROW_GAP))
            .pt(px(SIDEBAR_ACTION_ROW_GAP))
            .flex_none()
            .child(row)
    }

    fn start_available_update(&mut self, cx: &mut Context<Self>) {
        if self.updater_status != crate::updater::UpdateStatus::Available {
            return;
        }
        let started = cx
            .try_global::<crate::updater::UpdaterState>()
            .and_then(|state| state.0.as_ref())
            .is_some_and(|updater| updater.install_available_update());
        if started {
            self.updater_status = crate::updater::UpdateStatus::Updating;
            self.reset_updater_button_animation();
            cx.notify();
        }
    }

    fn render_updater_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let status = self.updater_status;
        if status == crate::updater::UpdateStatus::Idle {
            return None;
        }

        let theme = Theme::current(cx);
        let foreground = rgb(0xFFFFFF).into();
        let available = status == crate::updater::UpdateStatus::Available;
        let button = div()
            .id("sidebar-update")
            .track_focus(&self.updater_button_focus)
            .when(available, |button| button.tab_index(0))
            .w(px(UPDATER_BUTTON_COLLAPSED_WIDTH))
            .h(px(20.0))
            .flex_none()
            .overflow_hidden()
            .rounded_full()
            .relative()
            .cursor_default()
            .bg(theme.gauge)
            .text_color(foreground)
            .text_size(sp(12.5))
            .font_weight(FontWeight::MEDIUM)
            .when(available, |button| {
                button
                    .hover(|style| style.opacity(0.92))
                    .focus_visible(|style| style.border(hairline()).border_color(rgb(0xFFFFFF)))
                    .active(|style| style.opacity(0.8))
                    .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
                        this.set_updater_button_hovered(*hovering, cx);
                    }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.start_available_update(cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.start_available_update(cx);
                            cx.stop_propagation();
                        }
                    }))
            });

        if !available {
            let indicator = motion::spin_slow(icon("icons/loader-circle.svg", 14.0, foreground));
            return Some(
                button
                    .tooltip(Tooltip::text(tr!("updater.updating")))
                    .child(
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(indicator),
                    )
                    .into_any_element(),
            );
        }

        let label: SharedString = tr_cow!("updater.update").into();
        let animation_generation = self.updater_button_animation_generation;
        if animation_generation == 0 {
            return Some(
                button
                    .child(updater_button_available_content(foreground, label, 0.0))
                    .into_any_element(),
            );
        }

        let from_width = self.updater_button_animation_from_width;
        let from_reveal = self.updater_button_animation_from_reveal;
        let target_width = if self.updater_button_expanded() {
            UPDATER_BUTTON_EXPANDED_WIDTH
        } else {
            UPDATER_BUTTON_COLLAPSED_WIDTH
        };
        let target_reveal = if self.updater_button_expanded() {
            1.0
        } else {
            0.0
        };
        let current_width = self.updater_button_width.clone();
        let current_reveal = self.updater_button_label_reveal.clone();

        Some(
            button
                .with_animation(
                    SharedString::from(format!("sidebar-updater-expand-{animation_generation}")),
                    Animation::new(Duration::from_millis(150)).with_easing(ease_out_quint()),
                    move |button, delta| {
                        let width = from_width + (target_width - from_width) * delta;
                        let reveal = from_reveal + (target_reveal - from_reveal) * delta;
                        current_width.set(width);
                        current_reveal.set(reveal);
                        button.w(px(width)).child(updater_button_available_content(
                            foreground,
                            label.clone(),
                            reveal,
                        ))
                    },
                )
                .into_any_element(),
        )
    }

    fn render_sidebar_footer(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        div()
            .id("sidebar-footer")
            // Hovering anywhere across the bottom strip — not just the
            // settings cog — raises the quick-action dock.
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.sidebar_dock_zone_hovered = *hovered;
                cx.notify();
            }))
            .flex_none()
            .h(px(40.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .child(
                div()
                    .id("open-settings")
                    .tab_index(0)
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .w(px(26.0))
                    .h(px(26.0))
                    .flex_none()
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .tooltip(Tooltip::text_with_action(
                        tr_cow!("common.settings"),
                        &OpenSettings,
                    ))
                    .child(icon("icons/settings.svg", 14.0, theme.text_tertiary))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_settings_action(&OpenSettings, window, cx);
                    })),
            )
            .child(
                div()
                    .id("open-shortcuts")
                    .tab_index(0)
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .w(px(26.0))
                    .h(px(26.0))
                    .flex_none()
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .tooltip(Tooltip::text(tr!("shortcuts.title")))
                    .child(icon("icons/keyboard.svg", 14.0, theme.text_tertiary))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_shortcuts(window, cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            this.open_shortcuts(window, cx);
                            cx.stop_propagation();
                        }
                    })),
            )
            .when_some(self.render_transfer_indicator(cx), |footer, ring| {
                footer.child(ring)
            })
            .child(div().flex_1())
            .when_some(self.render_updater_button(cx), |footer, button| {
                footer.child(button)
            })
    }

    /// The quick-action dock that rises above the footer while the sidebar's
    /// bottom strip is hovered. It keeps its own hover state so the pointer
    /// can cross from the footer onto it without flicker.
    fn render_sidebar_dock(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.sidebar_dock_zone_hovered && !self.sidebar_dock_hovered {
            return None;
        }
        let theme = Theme::current(cx);
        let mut items = vec![
            SidebarDockItem::Inbox,
            SidebarDockItem::Archive,
            SidebarDockItem::Shortcuts,
            SidebarDockItem::Settings,
        ];
        if self.state.friends_enabled {
            items.insert(0, SidebarDockItem::Friends);
        }
        Some(
            div()
                .id("sidebar-dock")
                .absolute()
                .bottom(px(40.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    this.sidebar_dock_hovered = *hovered;
                    if !*hovered {
                        this.sidebar_dock_hover_item = None;
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(px(2.0))
                        .children(
                            items
                                .iter()
                                .map(|item| self.render_sidebar_dock_item(*item, &theme, cx)),
                        ),
                )
                .into_any_element(),
        )
    }

    #[track_caller]
    fn render_sidebar_dock_item(
        &self,
        item: SidebarDockItem,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (id, path, label) = match item {
            SidebarDockItem::Friends => ("friends", "icons/friends.svg", tr!("settings.friends")),
            SidebarDockItem::Inbox => ("inbox", "icons/inbox.svg", tr!("sidebar.inbox")),
            SidebarDockItem::Archive => ("archive", "icons/archive.svg", tr!("settings.archived")),
            SidebarDockItem::Shortcuts => {
                ("shortcuts", "icons/keyboard.svg", tr!("shortcuts.title"))
            }
            SidebarDockItem::Settings => (
                "settings",
                "icons/settings-hexagon.svg",
                tr!("common.settings"),
            ),
        };
        let hovered = self.sidebar_dock_hover_item == Some(item);
        // Sketch "Dock": white buttons cooling to pale blue at the bottom, a
        // soft blue shadow, a 12% black hairline, and solid black glyphs.
        let surface = linear_gradient(
            180.0,
            linear_color_stop(rgb(0xFFFFFF), 0.0),
            linear_color_stop(rgb(0xC4DCFC), 1.0),
        );
        let pill_surface = linear_gradient(
            180.0,
            linear_color_stop(rgb(0xFFFFFF), 0.0),
            linear_color_stop(rgb(0xEDF5FF), 1.0),
        );
        let surface_border = gpui::hsla(0.0, 0.0, 0.0, 0.12);
        let surface_shadow = vec![gpui::BoxShadow {
            color: rgb(0xDAEAFF).into(),
            offset: point(px(0.0), px(1.0)),
            blur_radius: px(2.0),
            spread_radius: px(0.0),
            inset: false,
        }];
        let glyph: Hsla = rgb(0x000000).into();
        div()
            .id(SharedString::from(format!("sidebar-dock-{id}")))
            .tab_index(0)
            .flex()
            .flex_col()
            .items_center()
            .w(px(48.0))
            .cursor_default()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.sidebar_dock_hover_item = Some(item);
                } else if this.sidebar_dock_hover_item == Some(item) {
                    this.sidebar_dock_hover_item = None;
                }
                cx.notify();
            }))
            .child(
                div()
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .when(hovered, |slot| {
                        slot.child(
                            div()
                                .h(px(18.0))
                                .px(px(10.0))
                                .rounded_full()
                                .bg(pill_surface)
                                .border(hairline())
                                .border_color(surface_border)
                                .shadow(surface_shadow.clone())
                                .flex()
                                .items_center()
                                .text_size(sp(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(glyph)
                                .child(label),
                        )
                    }),
            )
            .child(
                div()
                    .size(px(44.0))
                    .rounded_full()
                    .bg(surface)
                    .border(hairline())
                    .border_color(surface_border)
                    .shadow(surface_shadow)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(path, 22.0, glyph)),
            )
            .focus_visible(|style| {
                style
                    .border(hairline())
                    .border_color(theme.accent)
                    .rounded(px(12.0))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.activate_sidebar_dock_item(item, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    this.activate_sidebar_dock_item(item, window, cx);
                    cx.stop_propagation();
                }
            }))
    }

    fn activate_sidebar_dock_item(
        &mut self,
        item: SidebarDockItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item {
            SidebarDockItem::Friends => {
                self.open_settings_page(SettingsPage::Friends, window, cx)
            }
            SidebarDockItem::Inbox => {
                if self.notifications.open {
                    self.close_inbox(cx);
                } else {
                    self.open_inbox(window, cx);
                }
            }
            SidebarDockItem::Archive => {
                self.open_settings_page(SettingsPage::Archived, window, cx)
            }
            SidebarDockItem::Shortcuts => self.open_shortcuts(window, cx),
            SidebarDockItem::Settings => self.open_settings_action(&OpenSettings, window, cx),
        }
    }

    /// The footer keyboard button's target: the keybindings manager when it
    /// is enabled, otherwise the legacy shortcuts dialog, whose focus lands
    /// two frames after the modal joins the dispatch tree.
    fn open_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if crate::keybindings::manager_enabled() {
            self.open_keybindings_page(window, cx);
            return;
        }
        let focus = self.open_shortcuts_dialog(cx);
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        });
    }

    /// Aggregate in-flight friend transfers into one footer ring beside the
    /// shortcuts button — hidden when idle. The arc tracks summed bytes when
    /// offers declared a size; until then a spinning loader carries the
    /// state. Activating it lands on Settings → Friends.
    fn render_transfer_indicator(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.state.friends_enabled {
            return None;
        }
        let active = self
            .friends_state
            .transfers
            .iter()
            .filter(|t| {
                matches!(
                    t.status,
                    TransferStatus::Pending | TransferStatus::Transferring
                )
            })
            .collect::<Vec<_>>();
        if active.is_empty() {
            return None;
        }
        let theme = Theme::current(cx);
        let total: u64 = active.iter().map(|t| t.bytes_total).sum();
        let done: u64 = active.iter().map(|t| t.bytes_done).sum();
        let percent = (total > 0).then_some(done as f64 * 100.0 / total as f64);
        let glyph: AnyElement = match percent {
            Some(percent) => {
                progress_ring(Some(percent), theme.border_strong, theme.accent)
                    .into_any_element()
            }
            None => motion::spin_slow(icon("icons/loader-circle.svg", 13.0, theme.accent)),
        };
        let count = active.len();
        Some(
            div()
                .id("transfer-progress")
                .tab_index(0)
                .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                .w(px(26.0))
                .h(px(26.0))
                .flex_none()
                .rounded(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_default()
                .hover(|element| element.bg(theme.overlay))
                .active(|element| element.bg(theme.overlay_strong))
                .tooltip(Tooltip::text(tr!("friends.transfers_active", count = count)))
                .child(glyph)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_settings_page(SettingsPage::Friends, window, cx);
                }))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if !event.keystroke.modifiers.modified()
                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    {
                        this.open_settings_page(SettingsPage::Friends, window, cx);
                        cx.stop_propagation();
                    }
                }))
                .into_any_element(),
        )
    }

    /// Resolve every ordinary local project's branch in one background pass.
    /// The render path only computes an allocation-free source fingerprint;
    /// collection building and daemon requests happen once when that moves.
    fn ensure_sidebar_branch_labels(&self, cx: &mut Context<Self>) {
        if self.state.sidebar_grouping != SidebarGrouping::Project {
            return;
        }

        let mut fingerprint = 0xb4a7_c4e5_51de_ba11;
        for session in &self.state.sessions {
            if session.has_started() && matches!(&session.workspace, SessionWorkspace::Local) {
                fingerprint = mix_uuid(fingerprint, session.id);
                fingerprint = mix_uuid(fingerprint, session.project_id);
            }
        }
        for project in &self.state.projects {
            fingerprint = mix_uuid(fingerprint, project.id);
        }
        if self.sidebar_branch_scan_fingerprint.get() == Some(fingerprint) {
            return;
        }
        self.sidebar_branch_scan_fingerprint.set(Some(fingerprint));
        let generation = self.sidebar_branch_scan_generation.get().wrapping_add(1);
        self.sidebar_branch_scan_generation.set(generation);

        // Each daemon scans its own project paths; a host that is down keeps
        // its last-known labels until it answers again.
        let active_project_ids = self
            .state
            .sessions
            .iter()
            .filter(|session| {
                session.has_started() && matches!(&session.workspace, SessionWorkspace::Local)
            })
            .map(|session| session.project_id)
            .collect::<HashSet<_>>();
        let projectless_root = crate::projectless::workspace_root();
        let mut by_owner: HashMap<waku_client::DaemonKey, Vec<PathBuf>> = HashMap::new();
        let mut all_paths = HashSet::new();
        for project in &self.state.projects {
            if !active_project_ids.contains(&project.id)
                || sidebar_project_is_projectless(project, projectless_root.as_deref())
            {
                continue;
            }
            by_owner
                .entry(self.project_host(project.id))
                .or_default()
                .push(project.path.clone());
            all_paths.insert(project.path.clone());
        }
        if all_paths.is_empty() {
            self.sidebar_branch_labels.borrow_mut().clear();
            return;
        }

        // (online supervisors, their paths, paths owned by offline remotes)
        let mut scans = Vec::new();
        let mut offline = HashSet::new();
        for (owner, paths) in by_owner {
            match self.daemons.supervisor(owner) {
                Some(supervisor) => scans.push((supervisor, paths)),
                None => offline.extend(paths),
            }
        }
        cx.spawn(async move |waku, cx| {
            let labels = cx
                .background_executor()
                .spawn(async move {
                    let mut labels = HashMap::new();
                    for (supervisor, paths) in scans {
                        let workspace = waku_client::WorkspaceClient::new(supervisor.client());
                        for path in paths {
                            let branch = match workspace.request(
                                waku_client::WorkspaceOperation::InspectBranches {
                                    cwd: path.clone(),
                                },
                            ) {
                                Ok(waku_client::WorkspaceResult::Branches {
                                    snapshot: Some(snapshot),
                                }) => snapshot.display_branch().map(str::to_owned),
                                _ => None,
                            };
                            if let Some(branch) = branch {
                                labels.insert(path, branch);
                            }
                        }
                    }
                    labels
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if waku.sidebar_branch_scan_generation.get() != generation {
                    return;
                }
                let mut merged = waku.sidebar_branch_labels.borrow().clone();
                merged.retain(|path, _| offline.contains(path));
                merged.extend(
                    labels
                        .into_iter()
                        .map(|(path, branch)| (path, SharedString::from(branch))),
                );
                *waku.sidebar_branch_labels.borrow_mut() = merged;
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn cache_sidebar_branch_label(&self, path: &Path, branch: Option<&str>) {
        let mut labels = self.sidebar_branch_labels.borrow_mut();
        if let Some(branch) = branch.filter(|branch| !branch.is_empty()) {
            labels.insert(path.to_path_buf(), SharedString::from(branch.to_owned()));
        } else {
            labels.remove(path);
        }
    }

    /// Dirty flag + unpushed commit count for every started session's checkout
    /// or worktree, resolved in one background pass like the branch labels.
    /// Rows read only `sidebar_checkout_statuses`.
    fn ensure_sidebar_checkout_statuses(&self, cx: &mut Context<Self>) {
        // Group by owning daemon so remote checkouts resolve on their host.
        let mut by_owner: HashMap<waku_client::DaemonKey, Vec<PathBuf>> = HashMap::new();
        for session in &self.state.sessions {
            if !session.has_started() {
                continue;
            }
            let Some(path) = self.workspace_path_for_session(session) else {
                continue;
            };
            by_owner
                .entry(self.session_host(session.id))
                .or_default()
                .push(path.to_path_buf());
        }
        for paths in by_owner.values_mut() {
            paths.sort();
            paths.dedup();
        }

        // Sort by owner so the fingerprint is independent of map order.
        let mut owned_paths: Vec<(waku_client::DaemonKey, Vec<PathBuf>)> =
            by_owner.into_iter().collect();
        owned_paths.sort_by_key(|(owner, _)| *owner);
        let mut fingerprint = 0xc8ec_07a7_5a7a_5ca1;
        for (owner, paths) in &owned_paths {
            match owner {
                waku_client::DaemonKey::Local => fingerprint = mix(fingerprint, 0),
                waku_client::DaemonKey::Remote(host) => {
                    fingerprint = mix(fingerprint, 1);
                    fingerprint = mix_uuid(fingerprint, *host);
                }
            }
            for path in paths {
                for byte in path.as_os_str().as_encoded_bytes() {
                    fingerprint = mix(fingerprint, u64::from(*byte));
                }
                fingerprint = mix(fingerprint, 0xff);
            }
        }
        let rescan_due = self
            .sidebar_checkout_scanned_at
            .get()
            .is_none_or(|instant| instant.elapsed() >= SIDEBAR_CHECKOUT_STATUS_RESCAN);
        if self.sidebar_checkout_scan_fingerprint.get() == Some(fingerprint) && !rescan_due {
            return;
        }
        self.sidebar_checkout_scan_fingerprint
            .set(Some(fingerprint));
        self.sidebar_checkout_scanned_at.set(Some(Instant::now()));
        let generation = self.sidebar_checkout_scan_generation.get().wrapping_add(1);
        self.sidebar_checkout_scan_generation.set(generation);

        if owned_paths.is_empty() {
            self.sidebar_checkout_statuses.borrow_mut().clear();
            return;
        }

        let mut scans = Vec::new();
        let mut offline = HashSet::new();
        for (owner, paths) in owned_paths {
            match self.daemons.supervisor(owner) {
                Some(supervisor) => scans.push((supervisor, paths)),
                None => offline.extend(paths),
            }
        }
        cx.spawn(async move |waku, cx| {
            let statuses = cx
                .background_executor()
                .spawn(async move {
                    let mut statuses = HashMap::new();
                    for (supervisor, paths) in scans {
                        let workspace = waku_client::WorkspaceClient::new(supervisor.client());
                        for path in paths {
                            if let Ok(waku_client::WorkspaceResult::CheckoutStatus {
                                status: Some(status),
                            }) = workspace.request(
                                waku_client::WorkspaceOperation::InspectCheckoutStatus {
                                    cwd: path.clone(),
                                },
                            ) {
                                statuses.insert(path, status);
                            }
                        }
                    }
                    statuses
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if waku.sidebar_checkout_scan_generation.get() != generation {
                    return;
                }
                // Rows whose host is offline keep their last-known status.
                let mut merged = waku.sidebar_checkout_statuses.borrow().clone();
                merged.retain(|path, _| offline.contains(path));
                merged.extend(statuses);
                *waku.sidebar_checkout_statuses.borrow_mut() = merged;
                cx.notify();
            });
        })
        .detach();
    }

    /// Resolves each started session's pull requests on a background executor
    /// through the daemon, so a sidebar frame only ever reads
    /// `sidebar_pull_requests`. The fingerprint covers the inputs the scan
    /// uses — the session set, their workspaces and branches, and status
    /// transitions, since a finishing turn is the moment an agent's `gh pr
    /// create` lands — plus a slow time bucket that catches host-side changes
    /// (a review, a merge) no session event can see.
    fn ensure_sidebar_pull_requests(&self, cx: &mut Context<Self>) {
        const RESCAN_BUCKET_SECONDS: u64 = 300;

        // Experimental — no scan, and no PR badges, while the opt-in is off.
        if !self.state.github_enabled {
            return;
        }

        let mut fingerprint = 0xf1f9_9d5e_c7a3_b21d;
        // (session, checkout, worktree branch) grouped by owning daemon — gh
        // runs on whichever host holds the checkout.
        let mut targets: HashMap<waku_client::DaemonKey, Vec<(Uuid, PathBuf, Option<String>)>> =
            HashMap::new();
        for session in &self.state.sessions {
            if !session.has_started() || session.archived_at.is_some() {
                continue;
            }
            let (cwd, branch) = match &session.workspace {
                SessionWorkspace::Worktree { path, branch, .. } => (path.clone(), branch.clone()),
                SessionWorkspace::Local => match self
                    .state
                    .projects
                    .iter()
                    .find(|project| project.id == session.project_id)
                {
                    Some(project) if !project.is_projectless() => (project.path.clone(), None),
                    _ => continue,
                },
                SessionWorkspace::NewWorktree { .. } => continue,
            };
            let owner = self.session_host(session.id);
            fingerprint = mix_uuid(fingerprint, session.id);
            if let waku_client::DaemonKey::Remote(host) = owner {
                fingerprint = mix_uuid(fingerprint, host);
            }
            fingerprint = mix(fingerprint, sidebar_status_rank(session.status));
            match &branch {
                Some(branch) => fingerprint = mix_str(fingerprint, branch),
                None => fingerprint = mix(fingerprint, u64::MAX),
            }
            targets
                .entry(owner)
                .or_default()
                .push((session.id, cwd, branch));
        }
        fingerprint = mix(fingerprint, unix_time() / RESCAN_BUCKET_SECONDS);
        if self.sidebar_pull_request_scan_fingerprint.get() == Some(fingerprint) {
            return;
        }
        if targets.is_empty() {
            self.sidebar_pull_request_scan_fingerprint
                .set(Some(fingerprint));
            self.sidebar_pull_requests.borrow_mut().clear();
            return;
        }
        self.sidebar_pull_request_scan_fingerprint
            .set(Some(fingerprint));
        let generation = self
            .sidebar_pull_request_scan_generation
            .get()
            .wrapping_add(1);
        self.sidebar_pull_request_scan_generation.set(generation);

        let mut scans = Vec::new();
        let mut offline = HashSet::new();
        for (owner, sessions) in targets {
            match self.daemons.supervisor(owner) {
                Some(supervisor) => scans.push((supervisor, sessions)),
                None => offline.extend(sessions.iter().map(|(session_id, _, _)| *session_id)),
            }
        }
        cx.spawn(async move |waku, cx| {
            let resolved =
                cx.background_executor()
                    .spawn(async move {
                        let mut resolved = HashMap::new();
                        for (supervisor, targets) in scans {
                            let workspace = waku_client::WorkspaceClient::new(supervisor.client());
                            // Sessions on a shared checkout resolve their branch
                            // once per directory rather than once per session.
                            let mut branches: HashMap<PathBuf, Option<String>> = HashMap::new();
                            for (_, cwd, branch) in &targets {
                                if branch.is_none() && !branches.contains_key(cwd) {
                                    let resolved_branch = match workspace.request(
                                        waku_client::WorkspaceOperation::InspectBranches {
                                            cwd: cwd.clone(),
                                        },
                                    ) {
                                        Ok(waku_client::WorkspaceResult::Branches {
                                            snapshot: Some(snapshot),
                                        }) => snapshot.current,
                                        _ => None,
                                    };
                                    branches.insert(cwd.clone(), resolved_branch);
                                }
                            }
                            // And each (directory, branch) pair asks the host once.
                            let mut queries: HashMap<
                                (PathBuf, String),
                                Option<Vec<waku_client::PullRequestSummary>>,
                            > = HashMap::new();
                            for (session_id, cwd, branch) in targets {
                                let branch = branch
                                    .or_else(|| branches.get(&cwd).cloned().flatten())
                                    .filter(|branch| !branch.is_empty());
                                let Some(branch) = branch else {
                                    continue;
                                };
                                let entries =
                                    queries.entry((cwd.clone(), branch.clone())).or_insert_with(
                                        || match workspace.request(
                                            waku_client::WorkspaceOperation::ListPullRequests {
                                                cwd,
                                                head_branch: branch,
                                            },
                                        ) {
                                            Ok(waku_client::WorkspaceResult::PullRequests {
                                                entries: Some(entries),
                                            }) => Some(entries),
                                            _ => None,
                                        },
                                    );
                                if let Some(entries) = entries {
                                    resolved.insert(session_id, entries.clone());
                                }
                            }
                        }
                        resolved
                    })
                    .await;
            let _ = waku.update(cx, |waku, cx| {
                if waku.sidebar_pull_request_scan_generation.get() != generation {
                    return;
                }
                // Sessions whose host is offline keep their last-known rows.
                let mut merged = waku.sidebar_pull_requests.borrow().clone();
                merged.retain(|session_id, _| offline.contains(session_id));
                merged.extend(
                    resolved
                        .into_iter()
                        .map(|(session_id, entries)| (session_id, Rc::new(entries))),
                );
                *waku.sidebar_pull_requests.borrow_mut() = merged;
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_sidebar(
        &self,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        self.ensure_sidebar_branch_labels(cx);
        self.ensure_sidebar_checkout_statuses(cx);
        self.ensure_sidebar_pull_requests(cx);
        self.ensure_sidebar_terminal_repo_roots(cx);
        let is_resizing = self
            .panel_resize_drag
            .is_some_and(|drag| drag.target == PanelResizeTarget::Sidebar);

        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        self.sync_sidebar_rows(&rows);
        // Restored selection exists before ListState knows the viewport size.
        // Retry after the first layout so nearest-edge alignment has a height.
        if self.sidebar_list_state.viewport_bounds().size.height <= Pixels::ZERO
            && let Some(session_id) = self
                .pending_session_activation
                .map(|pending| pending.session_id)
                .or(self.state.selected_session)
        {
            let entity = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = entity.update(cx, |this, cx| {
                    let selected_session = this
                        .pending_session_activation
                        .map(|pending| pending.session_id)
                        .or(this.state.selected_session);
                    if selected_session == Some(session_id) {
                        this.reveal_sidebar_session(session_id);
                        cx.notify();
                    }
                });
            });
        }
        let history_scrolled =
            self.sidebar_list_state.scroll_px_offset_for_scrollbar().y < px(-0.5);
        let entity = cx.entity().downgrade();

        div()
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(if is_resizing {
                theme.sidebar_drag_background
            } else {
                theme.sidebar
            })
            .child(self.render_sidebar_titlebar(window, cx))
            .child(
                div()
                    .flex_none()
                    .px(px(10.0))
                    .child(self.render_sidebar_new_session(window, cx)),
            )
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div().px(px(10.0)).size_full().child(
                            list(self.sidebar_list_state.clone(), move |index, window, cx| {
                                entity
                                    .upgrade()
                                    .map(|entity| {
                                        entity.update(cx, |this, cx| {
                                            this.sidebar_row(index, &rows, window, cx)
                                        })
                                    })
                                    .unwrap_or_else(|| div().into_any_element())
                            })
                            .size_full(),
                        ),
                    )
                    // Rows dissolve into the sidebar surface just above the
                    // footer while more content waits below the fold.
                    .child(scrollbar::edge_fade(
                        self.sidebar_list_state.clone(),
                        scrollbar::FadeEdge::Bottom,
                        if is_resizing {
                            theme.sidebar_drag_background
                        } else {
                            theme.sidebar
                        },
                    ))
                    .child(scrollbar::vertical(
                        &self.sidebar_list_state,
                        &self.sidebar_scrollbar,
                    ))
                    .when(history_scrolled, |scroll| {
                        scroll.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .w_full()
                                .h(hairline())
                                .bg(theme.separator),
                        )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .relative()
                    .when_some(self.render_sidebar_dock(cx), |container, dock| {
                        container.child(dock)
                    })
                    .child(self.render_sidebar_footer(cx)),
            )
    }

    /// Keep a newly selected task visible without disturbing the sidebar when
    /// its row is already fully inside the viewport.
    pub(super) fn reveal_sidebar_session(&self, session_id: Uuid) {
        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        self.sync_sidebar_rows(&rows);
        if let Some(index) = sidebar_session_row_index(&rows, session_id) {
            reveal_sidebar_list_row(&self.sidebar_list_state, &rows, index);
        }
    }

    /// ⌘n — activate the nth task currently listed in the sidebar.
    pub(super) fn select_sidebar_session_action(
        &mut self,
        action: &SelectSidebarSession,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        let Some(session_id) = sidebar_shortcut_target_ids(&rows).nth(action.index) else {
            return;
        };
        // Mirror the click path: a pending inline rename commits before the
        // selection moves.
        if self.session_rename.is_some() {
            self.commit_session_rename(cx);
        }
        if self.terminal_rename.is_some() {
            self.commit_terminal_rename(cx);
        }
        self.settings_page = None;
        self.select_session(session_id, cx);
    }

    /// Arm or clear the ⌘-hold row chips as the primary modifier changes.
    /// The chips advertise a bare ⌘1–⌘9 chord, so any second modifier —
    /// held first or added mid-hold — keeps them hidden; releasing back to
    /// the bare modifier re-arms the delay. A hold that already spent ⌘ on
    /// a chord stays quiet until ⌘ comes all the way back up.
    pub(super) fn sidebar_shortcuts_modifiers_changed(
        &mut self,
        event: &gpui::ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_shortcut_hint_generation =
            self.sidebar_shortcut_hint_generation.wrapping_add(1);
        if !event.modifiers.secondary() {
            self.sidebar_shortcut_hint_chord_used = false;
        }
        if event.modifiers != gpui::Modifiers::secondary_key() || !self.state.sidebar_shortcut_tags
        {
            if self.sidebar_shortcut_hints {
                self.sidebar_shortcut_hints = false;
                cx.notify();
            }
            return;
        }
        if self.sidebar_shortcut_hints || self.sidebar_shortcut_hint_chord_used {
            return;
        }
        let generation = self.sidebar_shortcut_hint_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(SIDEBAR_SHORTCUT_HOLD_DELAY)
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.sidebar_shortcut_hint_generation != generation {
                    return;
                }
                this.sidebar_shortcut_hints = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// A ⌘-modified keystroke spends the current hold: the user already put
    /// the modifier to work, so the chips stay down until ⌘ comes back up.
    /// Registered on the capture phase so chords a binding claims count the
    /// same as unclaimed ones; a pending reveal timer is cancelled by the
    /// generation bump.
    pub(super) fn sidebar_shortcuts_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.keystroke.modifiers.secondary() {
            return;
        }
        self.sidebar_shortcut_hint_chord_used = true;
        self.sidebar_shortcut_hint_generation =
            self.sidebar_shortcut_hint_generation.wrapping_add(1);
        if self.sidebar_shortcut_hints {
            self.sidebar_shortcut_hints = false;
            cx.notify();
        }
    }

    /// Window deactivation delivers no modifiers-changed event, so ⌘-hold
    /// chips would stay painted while the app sits in the background.
    pub(super) fn sidebar_shortcuts_window_deactivated(&mut self, cx: &mut Context<Self>) {
        self.sidebar_shortcut_hint_generation =
            self.sidebar_shortcut_hint_generation.wrapping_add(1);
        self.sidebar_shortcut_hint_chord_used = false;
        if self.sidebar_shortcut_hints {
            self.sidebar_shortcut_hints = false;
            cx.notify();
        }
    }

    /// The sidebar row snapshot, rebuilt only when its inputs move.
    ///
    /// The sidebar re-renders at pulse cadence whenever one of its session
    /// rows shows a working spinner, and rebuilding the snapshot sorts every
    /// started session and runs calendar math per session — far too much per
    /// tick for values that move at most once per stream commit. The
    /// fingerprint is an allocation-free scan of exactly what
    /// [`Self::sidebar_rows`] reads: started sessions with their project and
    /// recency, the presentation preferences, the collapsed-group set, and
    /// today's date.
    pub(super) fn sidebar_rows_cached(&self, today: NaiveDate) -> Rc<Vec<SidebarRow>> {
        let mut fingerprint = mix(0x51de_ba5e_5eed_c0de, today.num_days_from_ce() as u64);
        fingerprint = mix(
            fingerprint,
            match self.state.sidebar_grouping {
                SidebarGrouping::Project => 1,
                SidebarGrouping::Date => 2,
            },
        );
        fingerprint = mix(
            fingerprint,
            match self.state.sidebar_ordering {
                SidebarOrdering::LastUpdated => 1,
                SidebarOrdering::LastCreated => 2,
            },
        );
        fingerprint = mix(fingerprint, u64::from(self.state.projects_page_enabled));
        fingerprint = mix(fingerprint, u64::from(self.state.github_enabled));
        for session in &self.state.sessions {
            if !session.has_started() || session.archived_at.is_some() {
                continue;
            }
            fingerprint = mix_uuid(fingerprint, session.id);
            fingerprint = mix_uuid(fingerprint, session.project_id);
            fingerprint = mix(fingerprint, sidebar_session_timestamp(session));
            fingerprint = mix(fingerprint, u64::from(session.pinned_at.is_some()));
        }
        if self.state.sidebar_grouping == SidebarGrouping::Project {
            for project in &self.state.projects {
                fingerprint = mix_uuid(fingerprint, project.id);
            }
            // A map has no stable iteration order; combine order-independently.
            let revealed =
                self.sidebar_project_reveal_counts
                    .iter()
                    .fold(0u64, |combined, (group, count)| {
                        combined.wrapping_add(group.mix_fingerprint(*count as u64))
                    });
            fingerprint = mix(
                mix(fingerprint, self.sidebar_project_reveal_counts.len() as u64),
                revealed,
            );
        }
        // A set has no stable iteration order; combine order-independently.
        let collapsed = self
            .sidebar_collapsed_groups
            .iter()
            .fold(0u64, |combined, group| {
                combined.wrapping_add(group.mix_fingerprint(0))
            });
        fingerprint = mix(
            mix(fingerprint, self.sidebar_collapsed_groups.len() as u64),
            collapsed,
        );
        // The Terminals group reads its ids in list order plus each row's
        // pin state — pinned rows survive the fold, so both feed the rows.
        for terminal_id in self.sidebar_terminal_ids() {
            fingerprint = mix_uuid(fingerprint, terminal_id);
            fingerprint = mix(
                fingerprint,
                u64::from(
                    self.terminal_records
                        .get(&terminal_id)
                        .is_some_and(|record| record.pinned),
                ),
            );
        }
        if self.sidebar_rows_fingerprint.get() != Some(fingerprint) {
            let (rows, collapsed_members) = self.sidebar_rows(today);
            *self.sidebar_rows_snapshot.borrow_mut() = Rc::new(rows);
            *self.sidebar_collapsed_group_members.borrow_mut() = Rc::new(collapsed_members);
            self.sidebar_rows_fingerprint.set(Some(fingerprint));
        }
        self.sidebar_rows_snapshot.borrow().clone()
    }

    /// Snapshot the session history as a flat list of lightweight rows under
    /// the current grouping and ordering preferences. Collapsed groups also
    /// report their member session ids so a folded header can aggregate the
    /// unread state of rows it hides.
    fn sidebar_rows(
        &self,
        today: NaiveDate,
    ) -> (Vec<SidebarRow>, HashMap<SidebarGroup, Vec<Uuid>>) {
        let mut sorted_sessions = self
            .state
            .sessions
            .iter()
            .filter(|session| session.has_started() && session.archived_at.is_none())
            .collect::<Vec<_>>();
        sort_sidebar_sessions(&mut sorted_sessions, self.state.sidebar_ordering);

        // The Projects row is experimental chrome — absent while its opt-in
        // is off.
        let mut rows = vec![SidebarRow::Search];
        if self.state.projects_page_enabled {
            rows.push(SidebarRow::Projects);
        }
        // The Inbox row rides the GitHub integration opt-in.
        if self.state.github_enabled {
            rows.push(SidebarRow::Inbox);
        }

        // The Terminals group sits between the search field and the session
        // history. Its header renders even with no terminals — expanding an
        // empty group is how a first global terminal is made — and folding
        // it keeps only the pinned rows.
        let terminals_collapsed = self
            .sidebar_collapsed_groups
            .contains(&SidebarGroup::Terminals);
        rows.push(SidebarRow::Header(SidebarGroup::Terminals));
        for terminal_id in self.sidebar_terminal_ids() {
            if !terminals_collapsed
                || self
                    .terminal_records
                    .get(&terminal_id)
                    .is_some_and(|record| record.pinned)
            {
                rows.push(SidebarRow::Terminal(terminal_id));
            }
        }
        rows.push(SidebarRow::GroupSpacer);

        // Pinned tasks lead the sidebar in both groupings, ordered by the same
        // recency the row displays — newest first, independent of the ordering
        // preference applied to the ordinary groups below.
        let mut pinned = sorted_sessions
            .iter()
            .filter(|session| session.pinned_at.is_some())
            .copied()
            .collect::<Vec<_>>();
        pinned.sort_by_key(|session| std::cmp::Reverse(sidebar_session_timestamp(session)));
        let pinned_ids = pinned.iter().map(|session| session.id).collect::<Vec<_>>();
        let mut collapsed_members = HashMap::new();
        let pinned_collapsed = self
            .sidebar_collapsed_groups
            .contains(&SidebarGroup::Pinned);
        append_sidebar_group_rows(
            &mut rows,
            SidebarGroup::Pinned,
            &pinned_ids,
            pinned_collapsed,
            false,
        );
        if pinned_collapsed {
            collapsed_members.insert(SidebarGroup::Pinned, pinned_ids);
        }
        sorted_sessions.retain(|session| session.pinned_at.is_none());

        match self.state.sidebar_grouping {
            SidebarGrouping::Date => {
                let grouped_sessions = date_sidebar_groups(&sorted_sessions, today);
                for date_group in SessionDateGroup::ALL {
                    let group = SidebarGroup::Date(date_group);
                    let collapsed = self.sidebar_collapsed_groups.contains(&group);
                    append_sidebar_group_rows(
                        &mut rows,
                        group,
                        &grouped_sessions[date_group.index()],
                        collapsed,
                        false,
                    );
                    if collapsed {
                        collapsed_members
                            .insert(group, grouped_sessions[date_group.index()].clone());
                    }
                }
            }
            SidebarGrouping::Project => {
                let projectless_root = crate::projectless::workspace_root();
                let projectless_project_ids = self
                    .state
                    .projects
                    .iter()
                    .filter(|project| {
                        sidebar_project_is_projectless(project, projectless_root.as_deref())
                    })
                    .map(|project| project.id)
                    .collect::<HashSet<_>>();
                for (group, sessions) in
                    project_sidebar_groups(&sorted_sessions, &projectless_project_ids)
                {
                    let collapsed = self.sidebar_collapsed_groups.contains(&group);
                    if collapsed {
                        collapsed_members.insert(group, sessions.clone());
                    }
                    let revealed_extra_sessions = self
                        .sidebar_project_reveal_counts
                        .get(&group)
                        .copied()
                        .unwrap_or_default();
                    let (visible_sessions, show_more) =
                        visible_project_sessions(&sessions, revealed_extra_sessions);
                    append_sidebar_group_rows(
                        &mut rows,
                        group,
                        &visible_sessions,
                        collapsed,
                        show_more,
                    );
                }
            }
        }
        let has_session_header = rows.iter().any(
            |row| matches!(row, SidebarRow::Header(group) if *group != SidebarGroup::Terminals),
        );
        if !has_session_header {
            // Keep the header actions visible while there is no history.
            let group = match self.state.sidebar_grouping {
                SidebarGrouping::Date => SidebarGroup::Date(SessionDateGroup::Today),
                SidebarGrouping::Project => {
                    let projectless_root = crate::projectless::workspace_root();
                    self.state
                        .selected_project
                        .and_then(|project_id| {
                            self.state
                                .projects
                                .iter()
                                .find(|project| project.id == project_id)
                        })
                        .or_else(|| self.state.projects.first())
                        .map(|project| {
                            if sidebar_project_is_projectless(project, projectless_root.as_deref())
                            {
                                SidebarGroup::Projectless
                            } else {
                                SidebarGroup::Project(project.id)
                            }
                        })
                        .unwrap_or(SidebarGroup::Projectless)
                }
            };
            rows.push(SidebarRow::Header(group));
        }
        (rows, collapsed_members)
    }

    /// Keep the virtualized list in sync with the current row snapshot.
    /// Rows are cheap values, so only the minimal changed suffix is spliced,
    /// preserving scroll position and measured heights across unrelated churn
    /// (e.g. the active session's `updated_at` bumping on every stream tick).
    fn sync_sidebar_rows(&self, rows: &[SidebarRow]) {
        let mut cached = self.sidebar_row_cache.borrow_mut();
        if cached.as_slice() == rows {
            return;
        }
        let prefix = cached
            .iter()
            .zip(rows.iter())
            .take_while(|(a, b)| a == b)
            .count();
        let old_count = cached.len();
        *cached = rows.to_vec();
        if old_count == 0 {
            self.sidebar_list_state
                .reset_with_uniform_height(rows.len(), px(SIDEBAR_SESSION_ROW_HEIGHT));
            // The offset persisted at quit can only land once rows exist.
            if let Some(offset) = self.pending_sidebar_scroll.take() {
                self.sidebar_list_state.scroll_to(offset);
            }
        } else {
            self.sidebar_list_state
                .splice(prefix..old_count, rows.len() - prefix);
            // Newly inserted rows have no measured height yet; give them the
            // uniform hint so the scrollbar keeps a correct total height.
            self.sidebar_list_state
                .clone()
                .with_uniform_item_height(px(SIDEBAR_SESSION_ROW_HEIGHT));
        }
    }

    /// The first not-busy session at or below `position` in the current
    /// sidebar order, wrapping to the top. `position` is the row index the
    /// just-archived session occupied, so the row that followed it now sits
    /// there.
    pub(super) fn next_sidebar_session_from_row(&self, position: usize) -> Option<Uuid> {
        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        next_sidebar_session_in_rows(&rows, position, |session_id| {
            self.state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .is_some_and(|session| !session.is_busy())
        })
    }

    /// ⌘-click: toggle `session_id` in the multi-selection without making it
    /// the active surface. The clicked row becomes the range anchor either
    /// way — even when the toggle removed it — matching Finder's pivot.
    fn toggle_sidebar_multi_selection(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if !self.sidebar_multi_selection.remove(&session_id) {
            self.sidebar_multi_selection.insert(session_id);
        }
        self.sidebar_multi_selection_anchor = Some(session_id);
        cx.notify();
    }

    /// ⌘⇧-click: grow the multi-selection to cover every session row between
    /// the anchor and `session_id` in the current sidebar order. The anchor
    /// is the last row a modified click touched, then the active session,
    /// and finally the clicked row itself.
    fn extend_sidebar_multi_selection(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        let Some(target) = sidebar_session_row_index(&rows, session_id) else {
            return;
        };
        let anchor = self
            .sidebar_multi_selection_anchor
            .or(self.state.selected_session)
            .and_then(|session_id| sidebar_session_row_index(&rows, session_id))
            .unwrap_or(target);
        let (lo, hi) = (anchor.min(target), anchor.max(target));
        for row in &rows[lo..=hi] {
            if let SidebarRow::Session(id) = row {
                self.sidebar_multi_selection.insert(*id);
            }
        }
        self.sidebar_multi_selection_anchor = Some(session_id);
        cx.notify();
    }

    /// Any unmodified left click and bare Escape land here; an empty set
    /// clears for free.
    pub(super) fn clear_sidebar_multi_selection(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_multi_selection.is_empty() {
            return;
        }
        self.sidebar_multi_selection.clear();
        self.sidebar_multi_selection_anchor = None;
        cx.notify();
    }

    /// The multi-selection's members in sidebar row order — the batch a row
    /// menu or session shortcut acts on. Members hidden by a folded group or
    /// an unrevealed "show more" window follow in session order: the set's
    /// contract is whole-set, not visible-rows-only.
    pub(super) fn sidebar_multi_selection_targets(&self) -> Vec<Uuid> {
        if self.sidebar_multi_selection.is_empty() {
            return Vec::new();
        }
        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        let mut targets: Vec<Uuid> = rows
            .iter()
            .filter_map(|row| match row {
                SidebarRow::Session(session_id)
                    if self.sidebar_multi_selection.contains(session_id) =>
                {
                    Some(*session_id)
                }
                _ => None,
            })
            .collect();
        for session in &self.state.sessions {
            if self.sidebar_multi_selection.contains(&session.id)
                && !targets.contains(&session.id)
            {
                targets.push(session.id);
            }
        }
        targets
    }

    /// The root's capture phase: a left mouse-down without the primary
    /// modifier ends the multi-selection before the click lands — including
    /// one inside an open row menu, whose item callbacks already captured
    /// their target set when the menu opened.
    pub(super) fn sidebar_multi_selection_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Left && !event.modifiers.secondary() {
            self.clear_sidebar_multi_selection(cx);
        }
    }

    fn sidebar_row(
        &self,
        index: usize,
        rows: &[SidebarRow],
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = rows.get(index) else {
            return div().into_any_element();
        };
        match *row {
            SidebarRow::Search => self.render_sidebar_search(window, cx).into_any_element(),
            SidebarRow::Projects => self.render_sidebar_projects(window, cx).into_any_element(),
            SidebarRow::Inbox => self.render_sidebar_inbox(window, cx).into_any_element(),
            SidebarRow::Header(group) => {
                let has_expanded_children = rows.get(index + 1).is_some_and(|row| {
                    matches!(
                        row,
                        SidebarRow::Session(_) | SidebarRow::Terminal(_) | SidebarRow::ShowMore(_)
                    )
                });
                // The header actions belong to the session history — the
                // Terminals group sits above it but never carries them.
                let first = group != SidebarGroup::Terminals
                    && !rows[..index].iter().any(|row| {
                        matches!(row, SidebarRow::Header(other) if *other != SidebarGroup::Terminals)
                    });
                self.render_sidebar_group_header(group, first, has_expanded_children, cx)
                    .into_any_element()
            }
            SidebarRow::Session(session_id) => {
                let shortcut_index = self
                    .sidebar_shortcut_hints
                    .then(|| {
                        sidebar_shortcut_target_ids(rows)
                            .take(SIDEBAR_SHORTCUT_TARGET_COUNT)
                            .position(|candidate| candidate == session_id)
                    })
                    .flatten();
                self.render_sidebar_session_item(session_id, shortcut_index, cx)
                    .into_any_element()
            }
            SidebarRow::Terminal(terminal_id) => self
                .render_sidebar_terminal_item(terminal_id, cx)
                .into_any_element(),
            SidebarRow::ShowMore(group) => {
                self.render_sidebar_show_more(group, cx).into_any_element()
            }
            SidebarRow::GroupSpacer => div()
                .w_full()
                .h(px(SIDEBAR_GROUP_SPACER_HEIGHT))
                .into_any_element(),
        }
    }

    fn render_sidebar_group_header(
        &self,
        group: SidebarGroup,
        first: bool,
        has_expanded_children: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let collapsed = self.sidebar_collapsed_groups.contains(&group);
        // A folded group keeps its hidden rows' unread signal: the same dot a
        // session or terminal row earns for an unseen completion surfaces on
        // the header. Pinned terminals stay visible through the fold, so they
        // report for themselves and are left out of the count.
        let has_unread = collapsed
            && match group {
                SidebarGroup::Terminals => {
                    self.unseen_terminal_completions.iter().any(|terminal_id| {
                        self.terminal_records
                            .get(terminal_id)
                            .is_some_and(|record| !record.pinned)
                            && !self.terminal_is_active_surface(*terminal_id)
                            && self.right_panel_terminals.get(terminal_id).is_some_and(
                                |terminal| {
                                    let terminal = terminal.read(cx);
                                    !terminal.command_running()
                                        && terminal.last_command_exit() == Some(0)
                                },
                            )
                    })
                }
                _ => self
                    .sidebar_collapsed_group_members
                    .borrow()
                    .get(&group)
                    .is_some_and(|members| {
                        members.iter().any(|session_id| {
                            self.state.unseen_completions.contains_key(session_id)
                                && self.state.sessions.iter().any(|session| {
                                    session.id == *session_id
                                        && session.status == SessionStatus::Idle
                                })
                        })
                    }),
            };
        let group_key = group.element_key();
        let group_name = SharedString::from(format!("sidebar-group-header-{group_key}"));
        let header_focus = self
            .sidebar_group_header_focuses
            .borrow_mut()
            .entry(group)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let show_group_icon = matches!(
            group,
            SidebarGroup::Project(_) | SidebarGroup::Projectless | SidebarGroup::Terminals
        );
        let group_icon = match group {
            SidebarGroup::Projectless => "icons/chat.svg",
            SidebarGroup::Terminals => "icons/terminal-prompt.svg",
            SidebarGroup::Project(project_id)
                if self
                    .state
                    .projects
                    .iter()
                    .any(|project| project.id == project_id && project.temporary) =>
            {
                // A temporary project keeps the clock badge whether the
                // group is expanded or not — the ephemeral mark outranks
                // the folder's open state.
                "icons/folder-clock.svg"
            }
            _ if collapsed => "icons/folder.svg",
            _ => "icons/folder-open.svg",
        };
        let label = match group {
            SidebarGroup::Pinned => tr!("sidebar.pinned"),
            SidebarGroup::Terminals => tr!("sidebar.terminals"),
            SidebarGroup::Date(group) => group.label(),
            SidebarGroup::Project(project_id) => self
                .state
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(Project::display_name)
                .unwrap_or_else(|| tr!("project.no_project_name")),
            SidebarGroup::Projectless => tr!("project.chat"),
        };
        let folder_missing =
            matches!(group, SidebarGroup::Project(id) if self.missing_projects.contains(&id));
        // Remote projects carry their host's name — and an offline marker
        // while that host is disconnected — so the merged catalog never
        // hides which machine a row belongs to.
        let host_badge = match group {
            SidebarGroup::Project(project_id) => match self.project_host(project_id) {
                waku_client::DaemonKey::Remote(host) => self.remote_host_name(host).map(|name| {
                    if self.remote_host_connected(host) {
                        format!("· {name}")
                    } else {
                        format!("· {name} · {}", tr!("sidebar.offline"))
                    }
                }),
                waku_client::DaemonKey::Local => None,
            },
            _ => None,
        };
        let updated_chevron = matches!(
            group,
            SidebarGroup::Date(_) | SidebarGroup::Pinned | SidebarGroup::Terminals
        )
        .then(|| {
            icon("icons/chevron-down.svg", 14.0, theme.text_secondary)
                .when(collapsed, |icon| {
                    icon.with_transformation(gpui::Transformation::rotate(gpui::percentage(0.75)))
                })
                .invisible()
                .group_hover(group_name.clone(), |icon| icon.visible())
        });
        let compose = show_group_icon.then(|| {
            let compose_focus = self
                .sidebar_group_compose_focuses
                .borrow_mut()
                .entry(group)
                .or_insert_with(|| cx.focus_handle())
                .clone();
            div()
                .w(px(20.0))
                .h(px(22.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "sidebar-group-compose-{group_key}"
                        )))
                        .track_focus(&compose_focus)
                        .tab_index(0)
                        .tab_stop(true)
                        .w_0()
                        .h(px(22.0))
                        .overflow_hidden()
                        .rounded(px(4.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_default()
                        .opacity(0.0)
                        .group_hover(group_name.clone(), |style| style.w(px(20.0)).opacity(1.0))
                        .focus_visible(|style| {
                            style
                                .w(px(20.0))
                                .opacity(1.0)
                                .border(hairline())
                                .border_color(theme.accent)
                        })
                        .hover(|style| style.bg(theme.overlay))
                        .active(|style| style.bg(theme.overlay_strong))
                        .tooltip(if group == SidebarGroup::Terminals {
                            Tooltip::text_with_hint(
                                tr!("right_panel.new_terminal"),
                                ShortcutHint::action(&NewTerminal),
                            )
                        } else {
                            // ⌘N is registered to the project switcher, which
                            // propagates the chord here when no draft can take
                            // it.
                            Tooltip::text_with_hint(
                                tr!("menu.new_task"),
                                ShortcutHint::action(&NewSession)
                                    .shadowed_by(&SwitchProjectForward),
                            )
                        })
                        .child(icon("icons/compose.svg", 14.0, theme.text_secondary))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.open_new_task_for_sidebar_group(group, window, cx);
                        }))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.open_new_task_for_sidebar_group(group, window, cx);
                                cx.stop_propagation();
                            }
                        })),
                )
        });

        // The Terminals group reads as a third action row, so it matches the
        // New Task and Search rows' metrics — height, padding, icon box,
        // gap, and regular label weight; every other group keeps the
        // section-header styling.
        let action_row = group == SidebarGroup::Terminals;
        let header = session_group_header(
            &theme,
            if action_row {
                SIDEBAR_ACTION_ROW_HEIGHT
            } else {
                SIDEBAR_GROUP_HEADER_HEIGHT
            },
        )
        .id(SharedString::from(format!(
            "sidebar-group-toggle-{group_key}"
        )))
        .track_focus(&header_focus)
        .tab_index(0)
        .tab_group()
        .tab_stop(true)
        .group(group_name)
        .relative()
        .w_full()
        .rounded(px(8.0))
        .cursor_default()
        .when(action_row, |element| {
            element
                .px(px(4.0))
                .rounded(px(9.0))
                .font_weight(FontWeight::NORMAL)
                .text_size(sp(14.0))
        })
        .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
        .hover(|style| style.bg(theme.sidebar_item_background))
        .active(|style| style.bg(theme.overlay_strong))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(22.0))
                .flex()
                .items_center()
                .gap(px(if action_row { 8.0 } else { 5.0 }))
                .when(show_group_icon, |element| {
                    if action_row {
                        element.child(
                            div()
                                .size(px(20.0))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(group_icon, 16.0, theme.text_secondary)),
                        )
                    } else {
                        element.child(icon(group_icon, 14.0, theme.text_secondary))
                    }
                })
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(2.0))
                        .child(div().min_w_0().truncate().child(label))
                        .when(folder_missing, |element| {
                            element.child(
                                div()
                                    .id(SharedString::from(format!("sidebar-missing-{group_key}")))
                                    .flex_none()
                                    .tooltip(Tooltip::text(tr!("project.folder_missing")))
                                    .child(icon("icons/alert.svg", 11.0, theme.warning)),
                            )
                        })
                        .when_some(host_badge, |element, badge| {
                            element.child(
                                div()
                                    .flex_none()
                                    .text_color(theme.text_tertiary)
                                    .child(badge),
                            )
                        })
                        .when_some(updated_chevron, |element, chevron| element.child(chevron))
                        .when(has_unread, |element| {
                            element.child(
                                div()
                                    .flex_none()
                                    .size(px(12.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(div().size(px(7.0)).rounded_full().bg(theme.info)),
                            )
                        }),
                )
                .child(div().flex_1()),
        )
        .when_some(compose, |element, compose| element.child(compose))
        .when(first, |element| {
            element.child(self.render_sidebar_header_actions(cx))
        })
        .when(
            show_group_icon && has_expanded_children && group != SidebarGroup::Terminals,
            |element| {
                element.child(
                    div()
                        .absolute()
                        .left(px(SIDEBAR_GROUP_GUIDE_X))
                        .top(px(19.0))
                        .bottom(px(-2.0))
                        .w(hairline())
                        .bg(theme.separator),
                )
            },
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            this.toggle_sidebar_group(group, window, cx);
        }))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            match event.keystroke.key.as_str() {
                "enter" | "space" => {
                    this.toggle_sidebar_group(group, window, cx);
                    cx.stop_propagation();
                }
                "left" if !collapsed => {
                    if group == SidebarGroup::Terminals {
                        this.collapse_terminals_group(window, cx);
                    } else {
                        this.set_sidebar_group_collapsed(group, true, cx);
                    }
                    cx.stop_propagation();
                }
                "right" if collapsed => {
                    this.toggle_sidebar_group(group, window, cx);
                    cx.stop_propagation();
                }
                _ => {}
            }
        }));

        div()
            .w_full()
            .when(action_row, |element| {
                element.pt(px(SIDEBAR_ACTION_ROW_GAP))
            })
            .pb(px(SIDEBAR_GROUP_HEADER_BOTTOM_GAP))
            .child(header)
    }

    fn open_new_task_for_sidebar_group(
        &mut self,
        group: SidebarGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        match group {
            SidebarGroup::Project(project_id) => self.select_project(project_id, cx),
            SidebarGroup::Projectless => self.create_projectless_session(cx),
            // The Terminals group's compose button opens a global terminal
            // in the home directory, expanding the group so the new row —
            // and the selection — is visible.
            SidebarGroup::Terminals => {
                self.set_sidebar_group_collapsed(SidebarGroup::Terminals, false, cx);
                if let Some(home) = dirs::home_dir()
                    && let Some(terminal_id) = self.create_terminal(home, None, None, cx)
                {
                    self.select_terminal(terminal_id, window, cx);
                }
                return;
            }
            SidebarGroup::Pinned | SidebarGroup::Date(_) => return,
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
    }

    fn render_sidebar_show_more(&self, group: SidebarGroup, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let group_key = group.element_key();
        let focus = self
            .sidebar_show_more_focuses
            .borrow_mut()
            .entry(group)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let button = div()
            .id(SharedString::from(format!("sidebar-show-more-{group_key}")))
            .track_focus(&focus)
            .tab_index(0)
            .tab_stop(true)
            .flex_none()
            .cursor_default()
            .text_size(sp(12.5))
            .text_color(theme.text_tertiary)
            .focus_visible(|style| style.text_color(theme.text))
            .hover(|style| style.text_color(theme.text))
            .child(tr!("sidebar.show_more"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.show_more_project_sessions(group, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.show_more_project_sessions(group, cx);
                    cx.stop_propagation();
                }
            }));

        div()
            .relative()
            .w_full()
            .h(px(SIDEBAR_SHOW_MORE_ROW_HEIGHT))
            .pl(px(SIDEBAR_GROUP_CHILD_PADDING))
            .flex()
            .items_center()
            .child(button)
            .child(
                div()
                    .absolute()
                    .left(px(SIDEBAR_GROUP_GUIDE_X))
                    .top_0()
                    .w(px(SIDEBAR_GROUP_CHILD_PADDING
                        - SIDEBAR_GROUP_GUIDE_X
                        - 4.0))
                    .h(px(15.0))
                    .border_l(hairline())
                    .border_b(hairline())
                    .rounded_bl(px(4.0))
                    .border_color(theme.separator),
            )
    }

    fn show_more_project_sessions(&mut self, group: SidebarGroup, cx: &mut Context<Self>) {
        let revealed = self.sidebar_project_reveal_counts.entry(group).or_default();
        *revealed = revealed.saturating_add(SIDEBAR_PROJECT_REVEAL_BATCH);
        self.sidebar_rows_fingerprint.set(None);
        cx.notify();
    }

    fn toggle_sidebar_group(
        &mut self,
        group: SidebarGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Opening the Terminals group is a selection, not just disclosure:
        // the last-shown terminal takes the main area, and folding it back
        // returns to the previous location.
        if group == SidebarGroup::Terminals {
            if self.sidebar_collapsed_groups.contains(&group) {
                self.expand_terminals_group(window, cx);
            } else {
                self.collapse_terminals_group(window, cx);
            }
            return;
        }
        let collapsed = !self.sidebar_collapsed_groups.contains(&group);
        self.set_sidebar_group_collapsed(group, collapsed, cx);
    }

    pub(super) fn collapse_all_sidebar_groups(&mut self, cx: &mut Context<Self>) {
        let groups = self
            .sidebar_rows_cached(Local::now().date_naive())
            .iter()
            .filter_map(|row| match row {
                SidebarRow::Header(group) => Some(*group),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut changed = false;
        for group in groups {
            changed |= self.sidebar_collapsed_groups.insert(group);
            changed |= self.sidebar_project_reveal_counts.remove(&group).is_some();
        }
        if changed {
            self.sidebar_rows_fingerprint.set(None);
            cx.notify();
        }
    }

    pub(super) fn set_sidebar_group_collapsed(
        &mut self,
        group: SidebarGroup,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        let collapse_changed = if collapsed {
            self.sidebar_collapsed_groups.insert(group)
        } else {
            self.sidebar_collapsed_groups.remove(&group)
        };
        let reveal_reset = collapsed && self.sidebar_project_reveal_counts.remove(&group).is_some();
        if collapse_changed || reveal_reset {
            self.sidebar_rows_fingerprint.set(None);
            cx.notify();
        }
    }

    fn set_sidebar_grouping(&mut self, grouping: SidebarGrouping, cx: &mut Context<Self>) {
        if self.state.sidebar_grouping == grouping {
            return;
        }
        self.state.sidebar_grouping = grouping;
        self.sidebar_rows_fingerprint.set(None);
        self.sidebar_branch_scan_fingerprint.set(None);
        self.sidebar_branch_scan_generation
            .set(self.sidebar_branch_scan_generation.get().wrapping_add(1));
        self.sidebar_list_state.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: Pixels::ZERO,
        });
        self.save();
        cx.notify();
    }

    fn set_sidebar_ordering(&mut self, ordering: SidebarOrdering, cx: &mut Context<Self>) {
        if self.state.sidebar_ordering == ordering {
            return;
        }
        self.state.sidebar_ordering = ordering;
        self.sidebar_rows_fingerprint.set(None);
        self.sidebar_list_state.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: Pixels::ZERO,
        });
        self.save();
        cx.notify();
    }

    fn begin_session_rename(
        &mut self,
        session_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(title) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(localized_session_title)
        else {
            return;
        };

        self.terminal_rename = None;
        self.session_rename = Some(session_id);
        self.session_rename_input.update(cx, |input, cx| {
            input.set_content(title, cx);
            input.select_all_text(cx);
        });
        let focus = self.session_rename_input.read(cx).focus();
        window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        cx.notify();
    }

    pub(super) fn commit_session_rename(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.session_rename.take() else {
            return;
        };
        let title = self
            .session_rename_input
            .read(cx)
            .content()
            .trim()
            .to_owned();
        let should_update = !title.is_empty()
            && self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .is_some_and(|session| session.title != title);
        if should_update
            && self
                .state
                .session_mut(session_id)
                .is_some_and(|session| session.set_title(&title))
        {
            self.save();
        }
        cx.notify();
    }

    fn cancel_session_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session_rename.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn render_sidebar_session_item(
        &self,
        session_id: Uuid,
        shortcut_index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return div().into_any_element();
        };
        let selected = sidebar_session_selected(
            self.state.selected_session,
            self.pending_session_activation
                .map(|pending| pending.session_id),
            session_id,
        );
        let multi_selected = self.sidebar_multi_selection.contains(&session_id);
        let pinned = session.pinned_at.is_some();
        // While a ⌘n chip overlays the row, its trailing elements hide so
        // nothing competes with the chip; the gradient fades the rest.
        let shortcut_hint = shortcut_index.is_some();
        // The Pinned group mixes projects, so its rows keep the flat layout
        // and project-name detail even while Project grouping is active.
        let grouped_by_project = self.state.sidebar_grouping == SidebarGrouping::Project && !pinned;
        let left_padding = if grouped_by_project {
            SIDEBAR_GROUP_CHILD_PADDING
        } else {
            8.0
        };
        let renaming = self.session_rename == Some(session_id);
        let waku = cx.entity().downgrade();
        let menu = self.menu_handle(format!("session-{session_id}"), cx);
        let row_focus = menu.trigger_focus_handle().clone();
        let keyboard_menu = menu.clone();
        let row = div()
            .id(SharedString::from(format!("session-{}", session.id)))
            .w_full()
            .min_w_0()
            .pl(px(left_padding))
            .pr(px(8.0))
            .py(px(7.0))
            .rounded(px(9.0))
            .cursor_default()
            // The multi-selection wears an accent wash so it never reads as
            // the active row's neutral highlight; hover and press deepen it
            // instead of dropping back to the single-selection tint.
            .when(multi_selected, |element| {
                element.bg(theme.accent.opacity(0.14))
            })
            .when(!multi_selected && selected, |element| {
                element.bg(theme.sidebar_item_background)
            })
            .hover(|element| {
                element.bg(if multi_selected {
                    theme.accent.opacity(0.2)
                } else {
                    theme.sidebar_item_background
                })
            })
            .active(|element| {
                element.bg(if multi_selected {
                    theme.accent.opacity(0.26)
                } else {
                    theme.sidebar_item_background
                })
            })
            .child(self.render_session_row_body(session_id, grouped_by_project, shortcut_hint, cx))
            .when(!renaming, |element| {
                let drag_title = SharedString::from(localized_session_title(session));
                element
                    .track_focus(&row_focus)
                    .tab_index(0)
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    // Dragging a row anywhere the composer is reachable
                    // stages a session-reference chip there; dropping it back
                    // on the session it already addresses is a no-op.
                    .on_drag(
                        composer::SidebarSessionDrag {
                            session_id,
                            title: drag_title,
                        },
                        move |drag, _, _, cx| {
                            cx.new(|_| composer::SidebarSessionDragView {
                                title: drag.title.clone(),
                            })
                        },
                    )
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        let key = event.keystroke.key.as_str();
                        if matches!(key, "enter" | "space") {
                            this.select_session(session_id, cx);
                            cx.stop_propagation();
                        } else if key == "f10" && event.keystroke.modifiers.shift {
                            keyboard_menu.open_context_menu(window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        let modifiers = event.modifiers();
                        if modifiers.secondary() && modifiers.shift {
                            this.extend_sidebar_multi_selection(session_id, cx);
                        } else if modifiers.secondary() {
                            this.toggle_sidebar_multi_selection(session_id, cx);
                        } else if modifiers.shift {
                            this.toggle_session_pin(session_id, cx);
                        } else {
                            this.select_session(session_id, cx);
                        }
                    }))
            });
        let row = if renaming {
            div()
                .w_full()
                .child(row)
                .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                    if this.session_rename == Some(session_id) {
                        this.commit_session_rename(cx);
                    }
                }))
                .into_any_element()
        } else {
            context_menu(
                div().w_full().child(row),
                SharedString::from(format!("session-menu-{session_id}")),
                &menu,
                move |cx| {
                    let rename_waku = waku.clone();
                    let pin_waku = waku.clone();
                    let unread_waku = waku.clone();
                    let copy_waku = waku.clone();
                    let move_waku = waku.clone();
                    let archive_waku = waku.clone();
                    let remove_waku = waku.clone();
                    // The batch settles at open: a menu on a member of the
                    // multi-selection acts on the whole set, while a menu on
                    // an outsider drops the set and acts on that row alone.
                    // Item callbacks run after the root's plain-click capture
                    // has cleared the set, so they close over the resolved
                    // list instead of re-reading it.
                    let (targets, local_workspace, any_movable, all_pinned) = waku
                        .update(cx, |waku, cx| {
                            let targets = if waku.sidebar_multi_selection.contains(&session_id) {
                                waku.sidebar_multi_selection_targets()
                            } else {
                                waku.clear_sidebar_multi_selection(cx);
                                vec![session_id]
                            };
                            let local_workspace = targets.iter().any(|target| {
                                waku.state
                                    .sessions
                                    .iter()
                                    .find(|session| session.id == *target)
                                    .is_some_and(|session| session.workspace.is_local())
                            });
                            let any_movable = targets
                                .iter()
                                .any(|target| waku.can_move_session_to_worktree(*target));
                            let all_pinned = targets.iter().all(|target| {
                                waku.state
                                    .sessions
                                    .iter()
                                    .find(|session| session.id == *target)
                                    .is_some_and(|session| session.pinned_at.is_some())
                            });
                            (targets, local_workspace, any_movable, all_pinned)
                        })
                        .unwrap_or((vec![session_id], false, false, pinned));
                    let pin_targets = targets.clone();
                    let unread_targets = targets.clone();
                    let copy_targets = targets.clone();
                    let move_targets = targets.clone();
                    let archive_targets = targets.clone();
                    let remove_targets = targets;
                    let mut items = vec![
                        // Rename stays single-target: the inline field it
                        // opens can only hold one title.
                        MenuItem::new(tr!("common.rename"), move |window, cx| {
                            let _ = rename_waku.update(cx, |waku, cx| {
                                waku.begin_session_rename(session_id, window, cx);
                            });
                        })
                        .icon("icons/pencil.svg"),
                        MenuItem::new(
                            if all_pinned {
                                tr!("session.unpin")
                            } else {
                                tr!("session.pin")
                            },
                            move |_, cx| {
                                let _ = pin_waku.update(cx, |waku, cx| {
                                    waku.set_sessions_pinned(&pin_targets, !all_pinned, cx)
                                });
                            },
                        )
                        .shortcut_action(&ToggleSessionPin)
                        .icon(if all_pinned {
                            "icons/pin-off.svg"
                        } else {
                            "icons/pin.svg"
                        }),
                        MenuItem::new(tr!("session.mark_unread"), move |_, cx| {
                            let _ = unread_waku.update(cx, |waku, cx| {
                                for target in &unread_targets {
                                    waku.mark_session_unread(*target, cx);
                                }
                            });
                        })
                        .shortcut_action(&MarkSessionUnread)
                        .icon("icons/eye-off.svg"),
                        MenuItem::new(tr!("session.copy_working_directory"), move |_, cx| {
                            let _ = copy_waku.update(cx, |waku, cx| {
                                waku.copy_sessions_working_directory(&copy_targets, cx);
                            });
                        })
                        .shortcut_action(&CopyWorkingDirectory)
                        .icon("icons/copy.svg"),
                    ];
                    if local_workspace {
                        items.push(
                            MenuItem::new(tr!("session.move_to_worktree"), move |_, cx| {
                                let _ = move_waku.update(cx, |waku, cx| {
                                    for target in &move_targets {
                                        waku.move_session_to_worktree(*target, None, cx);
                                    }
                                });
                            })
                            .icon("icons/fork.svg")
                            .disabled(!any_movable),
                        );
                    }
                    items.extend([
                        MenuItem::new(tr!("session.archive"), move |window, cx| {
                            let _ = archive_waku.update(cx, |waku, cx| {
                                for target in &archive_targets {
                                    waku.archive_session(*target, window, cx)
                                }
                            });
                        })
                        .shortcut_action(&ArchiveSession)
                        .icon("icons/archive.svg"),
                        MenuItem::Separator,
                        MenuItem::new(tr!("common.remove"), move |window, cx| {
                            let _ = remove_waku.update(cx, |waku, cx| {
                                for target in &remove_targets {
                                    waku.remove_session(*target, window, cx)
                                }
                            });
                        })
                        .icon("icons/trash.svg"),
                    ]);
                    items
                },
            )
        };

        div()
            .relative()
            .w_full()
            .pb(px(SIDEBAR_SESSION_ROW_GAP))
            .child(row)
            .when(grouped_by_project, |element| {
                element.child(
                    div()
                        .absolute()
                        .left(px(SIDEBAR_GROUP_GUIDE_X))
                        .top_0()
                        .bottom_0()
                        .w(hairline())
                        .bg(theme.separator),
                )
            })
            .when_some(shortcut_index, |element, index| {
                // `theme.sidebar` stays clear while vibrancy draws the real
                // surface, so the fade borrows the solid tint at reduced alpha
                // rather than covering the blur with an opaque patch.
                let fade = if theme.sidebar.a == 0.0 {
                    theme.sidebar_drag_background.opacity(0.85)
                } else {
                    theme.sidebar
                };
                element.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom(px(SIDEBAR_SESSION_ROW_GAP))
                        .right_0()
                        .flex()
                        .child(div().h_full().w(px(SIDEBAR_SHORTCUT_CHIP_FADE_WIDTH)).bg(
                            linear_gradient(
                                90.0,
                                linear_color_stop(fade.opacity(0.0), 0.0),
                                linear_color_stop(fade, 1.0),
                            ),
                        ))
                        .child(
                            div()
                                .h_full()
                                .pr(px(8.0))
                                .rounded_tr(px(9.0))
                                .rounded_br(px(9.0))
                                .bg(fade)
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .h(px(18.0))
                                        .px(px(5.0))
                                        .rounded(px(4.0))
                                        .border(hairline())
                                        .border_color(theme.border_subtle)
                                        .bg(theme.raised)
                                        .flex()
                                        .items_center()
                                        .text_size(sp(11.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text_secondary)
                                        .child(SharedString::from(sidebar_shortcut_chip_label(
                                            index,
                                        ))),
                                ),
                        ),
                )
            })
            .into_any_element()
    }

    /// The two-line body a session row shares between the sidebar and a Big
    /// Picture card header: title plus status/archive on top, project or
    /// branch detail below. `grouped_by_project` swaps the detail line into
    /// branch mode the way a project-grouped sidebar does; `shortcut_hint`
    /// hides the trailing controls while a ⌘n chip overlays the row.
    pub(super) fn render_session_row_body(
        &self,
        session_id: Uuid,
        grouped_by_project: bool,
        shortcut_hint: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return div().into_any_element();
        };
        let working = matches!(
            session.status,
            SessionStatus::Connecting | SessionStatus::Working
        );
        let project = self
            .state
            .projects
            .iter()
            .find(|project| project.id == session.project_id);
        let pinned = session.pinned_at.is_some();
        let detail_label = if grouped_by_project {
            persisted_sidebar_branch_label(&session.workspace)
                .map(|branch| SharedString::from(branch.to_owned()))
                .or_else(|| {
                    if !matches!(&session.workspace, SessionWorkspace::Local) {
                        return None;
                    }
                    project.and_then(|project| {
                        self.sidebar_branch_labels
                            .borrow()
                            .get(&project.path)
                            .cloned()
                    })
                })
        } else {
            Some(SharedString::from(
                if project.is_some_and(Project::is_projectless) {
                    tr!("project.chat")
                } else {
                    project
                        .map(Project::display_name)
                        .unwrap_or_else(|| tr!("sidebar.unknown_project"))
                },
            ))
        };
        // Date grouping and Big Picture cards have no project header to carry
        // the host name, so the detail line wears it. Project grouping leaves
        // it to the group header's badge.
        let (detail_label, session_remote) = match self.session_host(session_id) {
            waku_client::DaemonKey::Remote(host) if !grouped_by_project => {
                let host_name = self.remote_host_name(host);
                let label = match (detail_label, host_name) {
                    (Some(label), Some(host)) => format!("{label} · {host}"),
                    (None, Some(host)) => host,
                    (label, None) => label.map(|label| label.to_string()).unwrap_or_default(),
                };
                (Some(SharedString::from(label)), true)
            }
            _ => (detail_label, false),
        };
        let has_detail_label = detail_label.is_some();
        let checkout_status = if session.has_started() {
            self.workspace_path_for_session(session)
                .and_then(|path| self.sidebar_checkout_statuses.borrow().get(path).copied())
        } else {
            None
        };
        let detail_icon = if grouped_by_project {
            "icons/git-branch.svg"
        } else if session_remote {
            "icons/server.svg"
        } else if project.is_some_and(Project::is_projectless) {
            "icons/chat.svg"
        } else {
            "icons/folder.svg"
        };
        let rename_input =
            (self.session_rename == Some(session_id)).then(|| self.session_rename_input.clone());
        let title = if let Some(rename_input) = rename_input {
            div()
                .id(SharedString::from(format!(
                    "session-rename-field-{session_id}"
                )))
                .key_context(SESSION_RENAME_PARENT_CONTEXT)
                .on_action(cx.listener(|this, _: &CancelSessionRename, window, cx| {
                    this.cancel_session_rename(window, cx);
                }))
                .h(px(18.0))
                .flex_1()
                .min_w_0()
                .px(px(4.0))
                .rounded(px(4.0))
                .border(hairline())
                .border_color(theme.accent)
                .bg(theme.inset)
                .flex()
                .items_center()
                .text_size(sp(13.5))
                .text_color(theme.text)
                .child(rename_input)
                .into_any_element()
        } else {
            div()
                .id(SharedString::from(format!("session-title-{session_id}")))
                .flex_1()
                .min_w_0()
                .whitespace_normal()
                .line_clamp(1)
                .text_overflow(gpui::TextOverflow::Truncate("...".into()))
                .text_size(sp(13.5))
                .text_color(theme.text)
                .on_click(
                    cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                        if event.click_count() == 2 {
                            this.begin_session_rename(session_id, window, cx);
                            cx.stop_propagation();
                        }
                    }),
                )
                .child(SharedString::from(localized_session_title(session)))
                .into_any_element()
        };
        let pull_request_badge = self
            .sidebar_pull_requests
            .borrow()
            .get(&session_id)
            .and_then(|entries| {
                sidebar_pull_request_badge(entries, session_pull_request_window(session))
            });
        // The informational-blue dot an unread notification thread earns —
        // a status marker, not a control. The header chip owns interaction.
        let pull_request_unread = self
            .sidebar_pull_requests
            .borrow()
            .get(&session_id)
            .is_some_and(|entries| {
                session_pull_requests_in_window(entries, session_pull_request_window(session))
                    .iter()
                    .any(|entry| self.notifications.has_unread_pull_request(&entry.url))
            });
        let status_indicator: Option<AnyElement> = if shortcut_hint {
            None
        } else if working {
            Some(motion::spin_slow(icon(
                "icons/loader-circle.svg",
                12.0,
                status_color(&theme, session.status),
            )))
        } else {
            match session.status {
                SessionStatus::Background => Some(
                    icon(
                        "icons/hourglass.svg",
                        12.0,
                        status_color(&theme, session.status),
                    )
                    .into_any_element(),
                ),
                SessionStatus::Waiting => Some(
                    icon(
                        "icons/alert.svg",
                        12.0,
                        status_color(&theme, session.status),
                    )
                    .into_any_element(),
                ),
                SessionStatus::Failed => Some(
                    icon("icons/x.svg", 12.0, status_color(&theme, session.status))
                        .into_any_element(),
                ),
                SessionStatus::Idle if self.state.unseen_completions.contains_key(&session_id) => {
                    Some(
                        div()
                            .flex_none()
                            .size(px(12.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().size(px(7.0)).rounded_full().bg(theme.info))
                            .into_any_element(),
                    )
                }
                _ => None,
            }
        };
        let group_name = SharedString::from(format!("session-row-{session_id}"));
        let archive_focus = self
            .sidebar_session_archive_focuses
            .borrow_mut()
            .entry(session_id)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        // The archive control borrows the status slot: it stays zero-width
        // until the row is hovered or the button takes keyboard focus.
        let archive_button = div()
            .id(SharedString::from(format!("session-archive-{session_id}")))
            .track_focus(&archive_focus)
            .tab_index(0)
            .flex_none()
            .w_0()
            .h(px(18.0))
            .overflow_hidden()
            .rounded(px(4.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .opacity(0.0)
            .group_hover(group_name.clone(), |style| style.w(px(20.0)).opacity(1.0))
            .focus_visible(|style| {
                style
                    .w(px(20.0))
                    .opacity(1.0)
                    .border(hairline())
                    .border_color(theme.accent)
            })
            .hover(|style| style.bg(theme.overlay))
            .active(|style| style.bg(theme.overlay_strong))
            .tooltip(Tooltip::text_with_action(
                tr!("session.archive"),
                &ArchiveSession,
            ))
            .child(icon("icons/archive.svg", 12.0, theme.text_secondary))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.archive_session(session_id, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.archive_session(session_id, window, cx);
                    cx.stop_propagation();
                }
            }));
        let pin_focus = self
            .sidebar_session_pin_focuses
            .borrow_mut()
            .entry(session_id)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        // The pin control shares the archive control's reveal: zero-width
        // until the row is hovered or the button takes keyboard focus.
        let pin_button = div()
            .id(SharedString::from(format!("session-pin-{session_id}")))
            .track_focus(&pin_focus)
            .tab_index(0)
            .flex_none()
            .w_0()
            .h(px(18.0))
            .overflow_hidden()
            .rounded(px(4.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .opacity(0.0)
            .group_hover(group_name.clone(), |style| style.w(px(20.0)).opacity(1.0))
            .focus_visible(|style| {
                style
                    .w(px(20.0))
                    .opacity(1.0)
                    .border(hairline())
                    .border_color(theme.accent)
            })
            .hover(|style| style.bg(theme.overlay))
            .active(|style| style.bg(theme.overlay_strong))
            .tooltip(Tooltip::text_with_action(
                if pinned {
                    tr!("session.unpin")
                } else {
                    tr!("session.pin")
                },
                &ToggleSessionPin,
            ))
            .child(icon(
                if pinned {
                    "icons/pin-filled.svg"
                } else {
                    "icons/pin.svg"
                },
                12.0,
                theme.text_secondary,
            ))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.toggle_session_pin(session_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.toggle_session_pin(session_id, cx);
                    cx.stop_propagation();
                }
            }));
        div()
            .group(group_name.clone())
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .overflow_hidden()
                    .line_height(sp(18.0))
                    .child(title)
                    .when_some(status_indicator, |element, indicator| {
                        element.child(
                            div()
                                .flex_none()
                                .size(px(12.0))
                                // The zero-width pin/archive pair still
                                // claims its two flex gaps; pulling the slot
                                // right by that amount keeps the indicator's
                                // right edge flush with the timestamp below.
                                .mr(px(-12.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .group_hover(group_name.clone(), |style| style.invisible())
                                .child(indicator),
                        )
                    })
                    .when(!shortcut_hint, |element| {
                        element.child(pin_button).child(archive_button)
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .min_h(sp(15.0))
                    .text_size(sp(if grouped_by_project { 12.5 } else { 13.0 }))
                    .line_height(sp(15.0))
                    .when_some(detail_label, |element, label| {
                        element
                            .child(icon(detail_icon, 12.5, theme.text_tertiary))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .text_color(theme.text_tertiary)
                                    .child(div().min_w_0().truncate().child(label))
                                    .when(
                                        checkout_status
                                            .is_some_and(|status| status.uncommitted_changes),
                                        |element| {
                                            element.child(icon(
                                                "icons/asterisk.svg",
                                                12.0,
                                                theme.text_ghost,
                                            ))
                                        },
                                    ),
                            )
                            .when_some(
                                checkout_status
                                    .map(|status| status.unpushed_commits)
                                    .filter(|count| *count > 0),
                                |element, count| {
                                    element.child(
                                        div()
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .gap(px(2.0))
                                            .child(icon(
                                                "icons/arrow-up.svg",
                                                12.0,
                                                theme.text_tertiary,
                                            ))
                                            .child(
                                                div()
                                                    .text_size(sp(12.5))
                                                    .text_color(theme.text_tertiary)
                                                    .child(SharedString::from(count.to_string())),
                                            ),
                                    )
                                },
                            )
                            .child(div().flex_1())
                    })
                    .when(!has_detail_label, |element| element.child(div().flex_1()))
                    .when(
                        session.workspace.is_worktree() && !shortcut_hint,
                        |element| element.child(icon("icons/fork.svg", 11.0, theme.text_tertiary)),
                    )
                    .when_some(
                        pull_request_badge.filter(|_| !shortcut_hint),
                        |element, badge| {
                            let color = sidebar_pull_request_color(&theme, badge.state);
                            element.child(
                                div()
                                    .id(SharedString::from(format!("session-pr-{session_id}")))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(3.0))
                                    .when(pull_request_unread, |element| {
                                        element.child(
                                            div().size(px(5.0)).rounded_full().bg(theme.info),
                                        )
                                    })
                                    .child(icon(
                                        sidebar_pull_request_icon(badge.state),
                                        12.0,
                                        color,
                                    ))
                                    .child(div().text_size(sp(12.5)).text_color(color).child(
                                        if badge.others == 0 {
                                            format!("#{}", badge.number)
                                        } else {
                                            format!("#{} +{}", badge.number, badge.others)
                                        },
                                    ))
                                    .when_some(badge.check_status, |element, status| {
                                        element.child(icon(
                                            sidebar_check_status_icon(status),
                                            11.5,
                                            sidebar_check_status_color(&theme, status),
                                        ))
                                    })
                                    .when_some(badge.review_decision, |element, decision| {
                                        element.child(icon(
                                            sidebar_review_decision_icon(decision),
                                            11.5,
                                            sidebar_review_decision_color(&theme, decision),
                                        ))
                                    })
                                    .tooltip(Tooltip::text(if pull_request_unread {
                                        format!(
                                            "{} · {}",
                                            sidebar_pull_request_tooltip(&badge),
                                            tr!("notifications.new_activity")
                                        )
                                    } else {
                                        sidebar_pull_request_tooltip(&badge)
                                    })),
                            )
                        },
                    )
                    .when_some(
                        session_time_label(session, unix_time()).filter(|_| !shortcut_hint),
                        |element, label| {
                            element.child(
                                div()
                                    .flex_none()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_tertiary)
                                    .child(SharedString::from(label)),
                            )
                        },
                    ),
            )
            .into_any_element()
    }

    // ── Header ─────────────────────────────────────────────────────────────

    pub(super) fn render_header(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let session = self.selected_session();
        // The header's pull-request chip reads the same scan the sidebar
        // badge does; the sidebar's own ensure never runs while it is hidden.
        self.ensure_sidebar_pull_requests(cx);
        let title = if let Some(terminal_id) = self.selected_terminal {
            self.terminal_records
                .get(&terminal_id)
                .and_then(|record| record.custom_title.clone())
                .or_else(|| {
                    self.right_panel_terminals
                        .get(&terminal_id)
                        .map(|terminal| single_line_label(terminal.read(cx).title()))
                        .filter(|title| !title.is_empty())
                })
                .unwrap_or_else(|| tr!("right_panel.terminal"))
        } else if self.projects_page.is_some() {
            tr!("projects.title")
        } else {
            session
                .map(localized_session_title)
                .unwrap_or_else(|| tr!("session.new_task"))
        };
        let agent_preset_label = session
            .filter(|session| session.provider == ProviderKind::DeepSeek && session.has_started())
            .and_then(|session| self.agent_preset_label_for_session(session));
        let sandboxed = session.is_some_and(|session| session.sandboxed);
        let left_window_controls = (!self.sidebar_visible)
            .then(|| {
                self.render_client_window_controls(
                    super::window_chrome::WindowControlSide::Left,
                    window,
                    cx,
                )
            })
            .flatten();
        let right_window_controls = (!self.right_panel_slot_visible())
            .then(|| {
                self.render_client_window_controls(
                    super::window_chrome::WindowControlSide::Right,
                    window,
                    cx,
                )
            })
            .flatten();
        div()
            .id("window-header")
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .children(left_window_controls)
            // The header starts where the sidebar ends, so until the sidebar
            // is wide enough to host the traffic lights itself the header has
            // to clear them. Steady state with the sidebar open adds nothing;
            // a sidebar sliding in shrinks the inset as it takes the lights
            // over, which is what keeps the title from passing under them.
            .pl(if self.sidebar_visible {
                px(14.0 + (TRAFFIC_LIGHT_CLEARANCE - self.sidebar_rendered_width).max(0.0))
            } else {
                px(0.0)
            })
            .pr(px(14.0))
            .when(!self.sidebar_visible, |element| {
                element
                    .child(
                        self.window_drag_region(
                            div()
                                .id("header-traffic-light-drag-region")
                                .w(px(TRAFFIC_LIGHT_CLEARANCE - 8.0))
                                .h_full()
                                .flex_none(),
                            cx,
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(self.render_sidebar_toggle(cx))
                            .child(self.render_unseen_completion_bell(cx))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(2.0))
                                    .child(self.render_history_button(
                                        "navigate-back",
                                        "icons/arrow-left.svg",
                                        !self.session_navigation.back.is_empty(),
                                        true,
                                        cx,
                                    ))
                                    .child(self.render_history_button(
                                        "navigate-forward",
                                        "icons/arrow-right.svg",
                                        !self.session_navigation.forward.is_empty(),
                                        false,
                                        cx,
                                    )),
                            ),
                    )
            })
            .child(
                self.window_drag_region(
                    div()
                        .id("header-title-drag-region")
                        .h_full()
                        .min_w_0()
                        .flex_shrink(1.0)
                        .flex()
                        .items_center()
                        .gap(px(7.0))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(sp(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(SharedString::from(title)),
                        )
                        .children(agent_preset_label.map(|label| {
                            div()
                                .h(px(22.0))
                                .max_w(px(180.0))
                                .px(px(6.0))
                                .rounded(px(8.0))
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .bg(theme.overlay)
                                .text_size(sp(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text_secondary)
                                .child(icon("icons/bot.svg", 10.5, theme.text_tertiary))
                                .child(div().min_w_0().truncate().child(SharedString::from(label)))
                        }))
                        // The container glyph only — the same icon the access
                        // menu and its chip use, with the word on the tooltip.
                        .when(sandboxed, |element| {
                            element.child(
                                div()
                                    .id("sandboxed-badge")
                                    .h(px(22.0))
                                    .w(px(22.0))
                                    .rounded(px(8.0))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(theme.overlay)
                                    .child(icon("icons/container.svg", 11.0, theme.text_secondary))
                                    .tooltip(Tooltip::text(tr!("sandbox.badge"))),
                            )
                        }),
                    cx,
                ),
            )
            .child(
                self.window_drag_region(
                    div().id("header-center-drag-region").h_full().flex_1(),
                    cx,
                ),
            )
            .child(self.render_background_work_summary(cx))
            .when(!self.right_panel_slot_visible(), |element| {
                element
                    .when(self.fps_counter_visible, |element| {
                        element.child(self.render_fps_counter(cx))
                    })
                    .when(self.state.git_panel_enabled, |element| {
                        element.child(self.render_git_panel_toggle(cx))
                    })
                    .child(self.render_right_panel_toggle(cx))
            })
            .children(right_window_controls)
    }

    // ── Empty states ───────────────────────────────────────────────────────

    pub(super) fn render_empty_state(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        if self.selected_project().is_none() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .px_8()
                .pb(px(46.0))
                .child(goddard_logo(&theme))
                .child(
                    div()
                        .mt(px(16.0))
                        .text_size(sp(20.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(tr_cow!("onboarding.open_project_to_begin")),
                )
                .child(
                    div()
                        .mt(px(8.0))
                        .max_w(px(380.0))
                        .text_center()
                        .text_size(sp(12.5))
                        .line_height(sp(19.0))
                        .text_color(theme.text_tertiary)
                        .child(tr_cow!("onboarding.description")),
                )
                .child(
                    div()
                        .mt(px(20.0))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.0))
                        .tab_index(0)
                        .tab_group()
                        .tab_stop(false)
                        .child(
                            div()
                                .id("onboarding-add-project")
                                .track_focus(&self.onboarding_add_project_focus)
                                .tab_index(0)
                                .focus_visible(|style| {
                                    style.border(hairline()).border_color(theme.accent)
                                })
                                .h(px(32.0))
                                .px(px(14.0))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .cursor_default()
                                .bg(theme.inverse)
                                .text_color(theme.on_inverse)
                                .text_size(sp(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .hover(|element| element.opacity(0.9))
                                .active(|element| element.opacity(0.8))
                                .child(tr_cow!("onboarding.open_project_folder"))
                                .on_click(cx.listener(|this, _, _, cx| this.add_project(cx)))
                                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.add_project(cx);
                                        cx.stop_propagation();
                                    }
                                })),
                        )
                        .child(
                            div()
                                .id("onboarding-projectless")
                                .track_focus(&self.onboarding_projectless_focus)
                                .tab_index(1)
                                .focus_visible(|style| {
                                    style.border(hairline()).border_color(theme.accent)
                                })
                                .h(px(30.0))
                                .px(px(12.0))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .cursor_default()
                                .text_color(theme.text_secondary)
                                .text_size(sp(12.5))
                                .hover(|element| element.bg(theme.overlay))
                                .active(|element| element.bg(theme.overlay_strong))
                                .child(icon("icons/x.svg", 11.0, theme.text_tertiary))
                                .child(tr_cow!("project.no_project"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.create_projectless_session(cx);
                                }))
                                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.create_projectless_session(cx);
                                        cx.stop_propagation();
                                    }
                                })),
                        ),
                );
        }
        let selected_project_id = self.state.selected_project;
        let projectless_selected = self.selected_project().is_some_and(Project::is_projectless);
        let project_name = self
            .selected_project()
            .map(|project| {
                if project.is_projectless() {
                    tr!("project.without_a_project")
                } else {
                    project.display_name()
                }
            })
            .unwrap_or_else(|| tr!("project.your_project"));
        let project_options = self
            .state
            .projects
            .iter()
            .filter(|project| !project.is_projectless())
            .filter(|project| Some(project.id) == selected_project_id)
            .chain(
                self.state
                    .projects
                    .iter()
                    .filter(|project| !project.is_projectless())
                    .filter(|project| Some(project.id) != selected_project_id),
            )
            .map(|project| (project.id, project.display_name()))
            .collect::<Vec<_>>();
        let weak = cx.entity().downgrade();
        let handle = self.menu_handle("empty-state-project", cx);
        let sync_notice = self.render_sync_notice(cx);
        let project_selector = dropdown_menu(
            ProjectNameSelector::new("empty-state-project", project_name)
                .selected(handle.is_open()),
            "empty-state-project-menu",
            &handle,
            MenuAlign::BelowLeft,
            move |_| {
                let mut items = project_options
                    .clone()
                    .into_iter()
                    .map(|(project_id, project_name)| {
                        let weak = weak.clone();
                        MenuItem::new(project_name, move |_, cx| {
                            if Some(project_id) == selected_project_id {
                                return;
                            }
                            let _ = weak.update(cx, |this, cx| this.select_project(project_id, cx));
                        })
                        .selected(Some(project_id) == selected_project_id)
                    })
                    .collect::<Vec<_>>();
                if !items.is_empty() {
                    items.push(MenuItem::Separator);
                }
                let add_project_weak = weak.clone();
                items.push(
                    MenuItem::new(tr!("project.new_project"), move |_, cx| {
                        let _ = add_project_weak.update(cx, |this, cx| this.add_project(cx));
                    })
                    .icon("icons/folder-new.svg")
                    .shortcut_action(&NewProject),
                );
                let projectless_weak = weak.clone();
                items.push(
                    MenuItem::new(tr!("project.no_project"), move |_, cx| {
                        let _ = projectless_weak.update(cx, |this, cx| {
                            if !this.selected_project().is_some_and(Project::is_projectless) {
                                this.create_projectless_session(cx);
                            }
                        });
                    })
                    .icon("icons/x.svg")
                    .selected(projectless_selected),
                );
                items
            },
        );
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px_8()
            // Centering splits added space, so twice the header height drops
            // the hero by the bar's full height.
            .pt(px(HEADER_HEIGHT * 2.0))
            .pb(px(52.0))
            .child(goddard_logo(&theme))
            .child(
                div()
                    .mt(px(14.0))
                    .flex()
                    .items_baseline()
                    .text_size(sp(20.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .when(projectless_selected, |element| {
                        element.child(tr_cow!("onboarding.what_should_we_build"))
                    })
                    .when(!projectless_selected, |element| {
                        element
                            .child(tr_cow!("onboarding.what_should_we_build_in"))
                            .child(project_selector)
                            .child(tr_cow!("onboarding.question_mark"))
                    }),
            )
            .children(sync_notice)
    }
}

fn localized_session_title(session: &AgentSession) -> String {
    let title = session.display_title();
    if title == AgentSession::DEFAULT_TITLE {
        tr!("session.new_task")
    } else {
        title.to_owned()
    }
}

pub(super) fn sidebar_session_selected(
    selected_session: Option<Uuid>,
    pending_session: Option<Uuid>,
    session_id: Uuid,
) -> bool {
    pending_session.map_or(selected_session == Some(session_id), |pending| {
        pending == session_id
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_sessions_by_calendar_period() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
        let cases = [
            ((2026, 8, 12), SessionDateGroup::Today),
            ((2026, 8, 11), SessionDateGroup::Yesterday),
            ((2026, 8, 10), SessionDateGroup::ThisWeek),
            ((2026, 8, 1), SessionDateGroup::ThisMonth),
            ((2026, 1, 1), SessionDateGroup::ThisYear),
            ((2025, 12, 31), SessionDateGroup::More),
        ];

        for ((year, month, day), expected) in cases {
            let session_date = NaiveDate::from_ymd_opt(year, month, day).unwrap();
            assert_eq!(session_date_group_for_dates(session_date, today), expected);
        }
    }

    #[test]
    fn future_sessions_stay_in_today() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
        let tomorrow = NaiveDate::from_ymd_opt(2026, 8, 13).unwrap();
        assert_eq!(
            session_date_group_for_dates(tomorrow, today),
            SessionDateGroup::Today
        );
    }

    #[test]
    fn collapsed_sidebar_group_keeps_only_its_header_and_spacer() {
        let sessions = [Uuid::from_u128(1), Uuid::from_u128(2)];
        let group = SidebarGroup::Date(SessionDateGroup::Today);
        let mut expanded = Vec::new();
        append_sidebar_group_rows(&mut expanded, group, &sessions, false, false);
        assert_eq!(
            expanded,
            vec![
                SidebarRow::Header(group),
                SidebarRow::Session(sessions[0]),
                SidebarRow::Session(sessions[1]),
                SidebarRow::GroupSpacer,
            ]
        );

        let mut collapsed = Vec::new();
        append_sidebar_group_rows(&mut collapsed, group, &sessions, true, false);
        assert_eq!(
            collapsed,
            vec![SidebarRow::Header(group), SidebarRow::GroupSpacer,]
        );
    }

    #[test]
    fn shortcut_targets_follow_visible_row_order() {
        let sessions = (0..12u128).map(Uuid::from_u128).collect::<Vec<_>>();
        let rows = [
            vec![SidebarRow::Search, SidebarRow::Header(SidebarGroup::Pinned)],
            sessions.iter().copied().map(SidebarRow::Session).collect(),
            vec![SidebarRow::GroupSpacer],
        ]
        .concat();

        let targets = sidebar_shortcut_target_ids(&rows)
            .take(SIDEBAR_SHORTCUT_TARGET_COUNT)
            .collect::<Vec<_>>();
        assert_eq!(targets, sessions[..SIDEBAR_SHORTCUT_TARGET_COUNT]);
    }

    #[test]
    fn collapsed_groups_contribute_no_shortcut_targets() {
        let sessions = [Uuid::from_u128(1), Uuid::from_u128(2)];
        let mut rows = Vec::new();
        append_sidebar_group_rows(&mut rows, SidebarGroup::Pinned, &sessions, true, false);
        assert_eq!(sidebar_shortcut_target_ids(&rows).count(), 0);
    }

    #[test]
    fn hidden_project_sessions_keep_a_keyboard_reveal_row() {
        let group = SidebarGroup::Project(Uuid::from_u128(1));
        let mut expanded = Vec::new();
        append_sidebar_group_rows(&mut expanded, group, &[], false, true);
        assert_eq!(
            expanded,
            vec![
                SidebarRow::Header(group),
                SidebarRow::ShowMore(group),
                SidebarRow::GroupSpacer,
            ]
        );

        let mut collapsed = Vec::new();
        append_sidebar_group_rows(&mut collapsed, group, &[], true, true);
        assert_eq!(
            collapsed,
            vec![SidebarRow::Header(group), SidebarRow::GroupSpacer]
        );
    }

    #[test]
    fn project_sessions_reveal_history_beyond_the_default_cap() {
        let sessions = (1..=50).map(Uuid::from_u128).collect::<Vec<_>>();

        let (initial, show_more) = visible_project_sessions(&sessions, 0);
        assert_eq!(initial, sessions[..SIDEBAR_PROJECT_DEFAULT_VISIBLE]);
        assert!(show_more);

        let (first_batch, show_more) =
            visible_project_sessions(&sessions, SIDEBAR_PROJECT_REVEAL_BATCH);
        assert_eq!(
            first_batch,
            sessions[..SIDEBAR_PROJECT_DEFAULT_VISIBLE + SIDEBAR_PROJECT_REVEAL_BATCH]
        );
        assert!(show_more);

        let (all_sessions, show_more) =
            visible_project_sessions(&sessions, SIDEBAR_PROJECT_REVEAL_BATCH * 2);
        assert_eq!(all_sessions, sessions);
        assert!(!show_more);
    }

    #[test]
    fn sidebar_recency_uses_last_reply_with_creation_fallback() {
        let project_id = Uuid::new_v4();
        let mut renamed_old_session = AgentSession::new(project_id, ProviderKind::Codex);
        renamed_old_session.created_at = 10;
        renamed_old_session.last_reply_at = Some(20);
        renamed_old_session.updated_at = 1_000;

        let mut newer_unanswered_session = AgentSession::new(project_id, ProviderKind::Codex);
        newer_unanswered_session.created_at = 15;
        newer_unanswered_session.last_reply_at = None;
        newer_unanswered_session.updated_at = 15;

        assert_eq!(sidebar_session_timestamp(&renamed_old_session), 20);
        assert_eq!(sidebar_session_timestamp(&newer_unanswered_session), 15);

        let mut sessions = vec![&renamed_old_session, &newer_unanswered_session];
        sort_sidebar_sessions(&mut sessions, SidebarOrdering::LastUpdated);
        assert_eq!(sessions[0].id, renamed_old_session.id);

        sort_sidebar_sessions(&mut sessions, SidebarOrdering::LastCreated);
        assert_eq!(sessions[0].id, newer_unanswered_session.id);
    }

    #[test]
    fn date_groups_follow_last_updated_under_last_created_ordering() {
        use chrono::TimeZone;

        let today = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
        let local_epoch = |date: NaiveDate, hour: u32| -> u64 {
            Local
                .from_local_datetime(&date.and_hms_opt(hour, 0, 0).unwrap())
                .earliest()
                .unwrap()
                .timestamp() as u64
        };
        let yesterday = today.pred_opt().unwrap();

        let project_id = Uuid::new_v4();
        let mut replied_today = AgentSession::new(project_id, ProviderKind::Codex);
        replied_today.created_at = local_epoch(yesterday, 12);
        replied_today.last_reply_at = Some(local_epoch(today, 15));

        let mut newcomer = AgentSession::new(project_id, ProviderKind::Codex);
        newcomer.created_at = local_epoch(today, 14);
        newcomer.last_reply_at = None;

        let mut quiet_yesterday = AgentSession::new(project_id, ProviderKind::Codex);
        quiet_yesterday.created_at = local_epoch(yesterday, 11);
        quiet_yesterday.last_reply_at = None;

        let mut sessions = vec![&replied_today, &newcomer, &quiet_yesterday];
        sort_sidebar_sessions(&mut sessions, SidebarOrdering::LastCreated);

        let groups = date_sidebar_groups(&sessions, today);
        assert_eq!(
            groups[SessionDateGroup::Today.index()],
            vec![newcomer.id, replied_today.id]
        );
        assert_eq!(
            groups[SessionDateGroup::Yesterday.index()],
            vec![quiet_yesterday.id]
        );
    }

    #[test]
    fn project_grouping_preserves_global_group_and_session_order() {
        let first_project = Uuid::from_u128(1);
        let second_project = Uuid::from_u128(2);
        let first = AgentSession::new(first_project, ProviderKind::Codex);
        let second = AgentSession::new(second_project, ProviderKind::Codex);
        let third = AgentSession::new(first_project, ProviderKind::Codex);

        let groups = project_sidebar_groups(&[&second, &first, &third], &HashSet::new());

        assert_eq!(
            groups,
            vec![
                (SidebarGroup::Project(second_project), vec![second.id]),
                (
                    SidebarGroup::Project(first_project),
                    vec![first.id, third.id]
                ),
            ]
        );
    }

    #[test]
    fn projectless_sessions_share_one_trailing_group() {
        let ordinary_project = Uuid::from_u128(1);
        let first_projectless_project = Uuid::from_u128(2);
        let second_projectless_project = Uuid::from_u128(3);
        let first_projectless = AgentSession::new(first_projectless_project, ProviderKind::Codex);
        let ordinary = AgentSession::new(ordinary_project, ProviderKind::Codex);
        let second_projectless = AgentSession::new(second_projectless_project, ProviderKind::Codex);

        let groups = project_sidebar_groups(
            &[&first_projectless, &ordinary, &second_projectless],
            &HashSet::from([first_projectless_project, second_projectless_project]),
        );

        assert_eq!(
            groups,
            vec![
                (SidebarGroup::Project(ordinary_project), vec![ordinary.id]),
                (
                    SidebarGroup::Projectless,
                    vec![first_projectless.id, second_projectless.id]
                ),
            ]
        );
    }

    #[test]
    fn projectless_sidebar_projects_are_paths_under_the_workspace_root() {
        let root = Path::new("/tmp/.goddard/projects");
        let projectless = Project {
            id: Uuid::from_u128(1),
            name: "Task".to_owned(),
            path: root.join("2026-08-23/task"),
            bookmark: None,
            created_at: 0,
            temporary: false,
        };
        let ordinary = Project {
            id: Uuid::from_u128(2),
            name: "Ordinary".to_owned(),
            path: PathBuf::from("/tmp/dev/ordinary"),
            bookmark: None,
            created_at: 0,
            temporary: false,
        };

        assert!(sidebar_project_is_projectless(&projectless, Some(root)));
        assert!(!sidebar_project_is_projectless(&ordinary, Some(root)));
        assert!(!sidebar_project_is_projectless(&projectless, None));
    }

    #[test]
    fn persisted_worktree_branches_supply_sidebar_labels() {
        let local = SessionWorkspace::Local;
        let planned = SessionWorkspace::NewWorktree {
            base_branch: Some("develop".to_owned()),
        };
        let worktree = SessionWorkspace::Worktree {
            path: PathBuf::from("/tmp/worktree"),
            name: "my-worktree".to_owned(),
            branch: Some("feature/sidebar".to_owned()),
            base_branch: None,
        };

        assert_eq!(persisted_sidebar_branch_label(&local), None);
        assert_eq!(persisted_sidebar_branch_label(&planned), Some("develop"));
        assert_eq!(
            persisted_sidebar_branch_label(&worktree),
            Some("my-worktree")
        );
    }

    #[test]
    fn pull_request_badge_reports_aggregate_state_and_oldest_in_class() {
        use waku_client::PullRequestState;

        fn entry(
            number: u64,
            state: PullRequestState,
            is_draft: bool,
            created_at: Option<u64>,
        ) -> waku_client::PullRequestSummary {
            waku_client::PullRequestSummary {
                number,
                title: format!("title {number}"),
                url: format!("https://github.com/o/r/pull/{number}"),
                state,
                is_draft,
                base_branch: "main".to_owned(),
                created_at,
                updated_at: None,
                review_decision: None,
                check_status: None,
                additions: None,
                deletions: None,
                author: None,
                head_branch: None,
            }
        }

        let window = (100, Some(200));
        let open = |number| entry(number, PullRequestState::Open, false, Some(150));

        assert!(sidebar_pull_request_badge(&[], window).is_none());

        let badge = sidebar_pull_request_badge(&[open(7)], window).unwrap();
        assert_eq!(badge.state, SidebarPullRequestState::Open);
        assert_eq!(badge.number, 7);
        assert_eq!(badge.others, 0);

        // Any open wins the aggregate; the badge number comes from the open
        // class, not the lowest number overall.
        let badge = sidebar_pull_request_badge(
            &[
                entry(3, PullRequestState::Merged, false, Some(150)),
                open(9),
                open(5),
            ],
            window,
        )
        .unwrap();
        assert_eq!(badge.state, SidebarPullRequestState::Open);
        assert_eq!(badge.number, 5);
        assert_eq!(badge.others, 2);

        // Draft only when every entry is one.
        let badge = sidebar_pull_request_badge(
            &[
                entry(4, PullRequestState::Open, true, Some(150)),
                entry(6, PullRequestState::Open, true, Some(150)),
            ],
            window,
        )
        .unwrap();
        assert_eq!(badge.state, SidebarPullRequestState::Draft);
        assert_eq!(badge.number, 4);

        let badge = sidebar_pull_request_badge(
            &[
                entry(4, PullRequestState::Open, true, Some(150)),
                entry(6, PullRequestState::Closed, false, Some(150)),
            ],
            window,
        )
        .unwrap();
        assert_eq!(badge.state, SidebarPullRequestState::Open);
        assert_eq!(badge.number, 4);

        let badge = sidebar_pull_request_badge(
            &[
                entry(8, PullRequestState::Merged, false, Some(150)),
                entry(2, PullRequestState::Merged, false, Some(150)),
            ],
            window,
        )
        .unwrap();
        assert_eq!(badge.state, SidebarPullRequestState::Merged);
        assert_eq!(badge.number, 2);

        let badge = sidebar_pull_request_badge(
            &[
                entry(8, PullRequestState::Merged, false, Some(150)),
                entry(2, PullRequestState::Closed, false, Some(150)),
            ],
            window,
        )
        .unwrap();
        assert_eq!(badge.state, SidebarPullRequestState::Closed);
        assert_eq!(badge.number, 2);
    }

    #[test]
    fn pull_request_badge_only_counts_pull_requests_inside_the_session_window() {
        use waku_client::PullRequestState;

        fn entry(number: u64, created_at: Option<u64>) -> waku_client::PullRequestSummary {
            waku_client::PullRequestSummary {
                number,
                title: format!("title {number}"),
                url: format!("https://github.com/o/r/pull/{number}"),
                state: PullRequestState::Open,
                is_draft: false,
                base_branch: "main".to_owned(),
                created_at,
                updated_at: None,
                review_decision: None,
                check_status: None,
                additions: None,
                deletions: None,
                author: None,
                head_branch: None,
            }
        }

        // Before the first turn and after the last turn ended are somebody
        // else's pull requests even though they share the branch.
        let badge = sidebar_pull_request_badge(
            &[entry(3, Some(50)), entry(9, Some(500))],
            (100, Some(200)),
        );
        assert!(badge.is_none());

        let badge = sidebar_pull_request_badge(
            &[entry(3, Some(50)), entry(9, Some(150)), entry(4, Some(500))],
            (100, Some(200)),
        )
        .unwrap();
        assert_eq!(badge.number, 9);
        assert_eq!(badge.others, 0);

        // An in-flight turn leaves the window open, and a host that reports
        // no creation time is kept rather than dropped.
        let badge = sidebar_pull_request_badge(&[entry(9, Some(500)), entry(4, None)], (100, None))
            .unwrap();
        assert_eq!(badge.number, 4);
        assert_eq!(badge.others, 1);
    }

    #[test]
    fn pending_session_replaces_sidebar_selection_immediately() {
        let current = Uuid::from_u128(1);
        let pending = Uuid::from_u128(2);

        assert!(!sidebar_session_selected(
            Some(current),
            Some(pending),
            current
        ));
        assert!(sidebar_session_selected(
            Some(current),
            Some(pending),
            pending
        ));
        assert!(sidebar_session_selected(Some(current), None, current));
    }

    #[test]
    fn selected_session_uses_nearest_bottom_edge_for_an_unmeasured_lower_row() {
        let target = Uuid::from_u128(31);
        let group = SidebarGroup::Date(SessionDateGroup::Today);
        let mut rows = vec![SidebarRow::Search, SidebarRow::Header(group)];
        rows.extend((1..=40).map(|id| SidebarRow::Session(Uuid::from_u128(id))));
        rows.push(SidebarRow::GroupSpacer);

        let index = sidebar_session_row_index(&rows, target).unwrap();
        let offset = sidebar_bottom_aligned_offset(&rows, index, px(400.0));

        assert_eq!(index, 32);
        assert_eq!(offset.item_ix, 25);
        assert_eq!(offset.offset_in_item, px(16.0));
        let visible_height = rows[offset.item_ix..=index]
            .iter()
            .copied()
            .map(sidebar_row_height)
            .fold(Pixels::ZERO, |height, row| height + row)
            - offset.offset_in_item;
        assert_eq!(visible_height, px(400.0));
        assert_eq!(sidebar_session_row_index(&rows, Uuid::from_u128(41)), None);
    }
}
