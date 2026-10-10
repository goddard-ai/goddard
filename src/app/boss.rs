//! Desktop control plane for daemon-owned Boss roles.
use super::boss_moods::AVATAR_SOURCE_SIZE;
use super::*;
use crate::ui::ActivationExt;
use waku_client::DaemonKey;
use waku_client::boss::{
    BossFile, BossIdentity, BossOperation, BossPersona, BossPersonaUpsert, BossResult, BossState,
    MemoryOperation, PersonaDefaultAction, PersonaDefaultRole, PersonaPermissions,
    instruction_diff, shipped_persona_default, shipped_persona_revision,
};
use waku_protocol::boss::AvatarStyle;
use waku_protocol::custom_commands::CustomCommandIcon;

fn global_avatar_style_operation(avatar_style: AvatarStyle) -> BossOperation {
    BossOperation::SetAvatarStyle {
        session_id: None,
        avatar_style,
    }
}

fn sidebar_deliverable_action_slot(group_name: SharedString) -> Div {
    div()
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
        .group_hover(group_name, |style| style.w(px(20.0)).opacity(1.0))
}

fn sidebar_deliverable_unread_status_slot(
    group_name: SharedString,
    id: SharedString,
    indicator: impl IntoElement,
) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .group_hover(group_name, |style| style.invisible())
        .child(indicator)
}

/// The logical size a session-mention chip's avatar occupies in the
/// transcript — `ATOM_AVATAR_SCALE` of a body-text chip's height.
const MENTION_AVATAR_SIZE: f32 = 18.0;

/// Initial render plus two retries, shared by every surface requesting a key.
const AVATAR_MAX_ATTEMPTS: u8 = 3;

/// The raster budget across every seed and bucket. Requests come only from
/// surfaces bounded by what is on screen — sidebar and roster rows, goal
/// rows, summon cards, composer atoms — so the working set stays well under
/// this even on a large roster.
const AVATAR_CACHE_LIMIT: usize = 512;

/// Rasters are cached per display-size bucket because GPUI samples sprites
/// with a bilinear filter and no mipmaps: one shared 256px raster upscaled
/// into a 54px header loses its edges, and downscaled six-fold into a
/// mention chip aliases into hard pixel steps. Quantizing requested sizes
/// keeps the buckets per seed small while each raster lands near the
/// pixels it actually occupies.
fn avatar_bucket(size: f32) -> u32 {
    (size.ceil().max(1.0) as u32).div_ceil(8) * 8
}

/// The scale passed to `SvgRenderer::render_single_frame` for a bucket.
/// GPUI multiplies it by its smooth-SVG factor of 2, so the raster comes
/// out at `2 * bucket` device pixels — a 1:1 sample on Retina displays and
/// a clean 2:1 minification elsewhere.
fn avatar_scale(bucket: u32) -> f32 {
    bucket as f32 / AVATAR_SOURCE_SIZE
}

/// The Gaze style's `look` track — the schema's 6.4s drift between seated
/// glances, evaluated by hand since the app rasterizes a single frame.
const GAZE_LOOK_PERIOD: f32 = 6.4;

/// The style's `blink` track — one close per 4.4s cycle.
const GAZE_BLINK_PERIOD: f32 = 4.4;

/// `blink` squashes the eyes between 94% and 97% of its cycle. Swapping the
/// raster across that whole window keeps at least one closed frame on the
/// 15fps pulse while reading as the same ~130ms flick.
const GAZE_BLINK_CLOSED: (f32, f32) = (0.94, 0.97);

/// The `look` track's translateX keyframes — schema percentages as phase,
/// values in spacing units (hundredths of the canvas).
const GAZE_LOOK_X: &[(f32, f32)] = &[
    (0.00, 0.0),
    (0.16, 0.0),
    (0.24, -3.6),
    (0.38, -3.6),
    (0.46, 3.4),
    (0.60, 3.4),
    (0.68, 0.7),
    (0.82, 0.7),
    (0.90, 0.0),
    (1.00, 0.0),
];

/// The `look` track's translateY keyframes.
const GAZE_LOOK_Y: &[(f32, f32)] = &[
    (0.00, 0.0),
    (0.16, 0.0),
    (0.24, 0.9),
    (0.38, 0.9),
    (0.46, -0.7),
    (0.60, -0.7),
    (0.68, 1.7),
    (0.82, 1.7),
    (0.90, 0.0),
    (1.00, 0.0),
];

/// Whether a surface leases the pulse clock for this avatar: Gaze faces
/// only, and never under reduce-motion, which keeps the exact static
/// element.
fn gaze_animates(style: AvatarStyle, reduce_motion: bool) -> bool {
    style == AvatarStyle::Gaze && !reduce_motion
}

/// Piecewise smoothstep interpolation over `(phase, value)` keyframes —
/// the schema's `easeInOut` timing without its CSS runtime.
fn gaze_track(phase: f32, keys: &[(f32, f32)]) -> f32 {
    let phase = phase.fract();
    let mut previous = keys[0];
    for &key in &keys[1..] {
        if phase < key.0 {
            let t = ((phase - previous.0) / (key.0 - previous.0)).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            return previous.1 + (key.1 - previous.1) * t;
        }
        previous = key;
    }
    keys.last().map_or(0.0, |key| key.1)
}

/// The `look` offset in rendered pixels at `phase` of the drift cycle.
/// Values are spacing units — hundredths of the avatar canvas — with the
/// eye group's ~1× scale and ±11° tilt inside the body approximated away:
/// the drift tops out under a pixel, where that error is invisible.
fn gaze_look_offset(phase: f32, size: f32) -> (f32, f32) {
    let unit = size / 100.0;
    (
        gaze_track(phase, GAZE_LOOK_X) * unit,
        gaze_track(phase, GAZE_LOOK_Y) * unit,
    )
}

/// The `blink` swap window — raster-level truth for a ~130ms close.
fn gaze_blink_closed(phase: f32) -> bool {
    let phase = phase.fract();
    (GAZE_BLINK_CLOSED.0..=GAZE_BLINK_CLOSED.1).contains(&phase)
}

/// Per-seed `(look, blink)` phase offsets in `[0, 1)` so neighboring Gaze
/// faces neither glance nor blink in sync.
fn gaze_eye_shift(seed: &str) -> (f32, f32) {
    (
        (sidebar::mix_str(0, seed) % 1024) as f32 / 1024.0,
        (sidebar::mix_str(1, seed) % 1024) as f32 / 1024.0,
    )
}

/// The eye-group rasters a Gaze avatar splits into — the body without eyes,
/// the eye pair at its seated spot, and the pair mid-blink — each a
/// full-canvas raster that overlays at the avatar's rendered size.
struct GazeFaces {
    body: Arc<gpui::RenderImage>,
    eyes: Arc<gpui::RenderImage>,
    eyes_closed: Arc<gpui::RenderImage>,
}

/// The rasters one (seed, style, bucket) render job caches — the still every
/// surface paints, plus the split layers only the animated Gaze path reads.
struct AvatarFaces {
    still: Arc<gpui::RenderImage>,
    gaze: Option<Arc<GazeFaces>>,
}

/// One frame of the animated Gaze face — the body raster with the eye-group
/// raster over it, drifted by the `look` phase and swapped to its mid-blink
/// frame inside the `blink` window. Resting phases paint the same pixels
/// the still does.
fn gaze_eye_frame(faces: &GazeFaces, size: f32, elapsed: f32, shift: (f32, f32)) -> AnyElement {
    let (dx, dy) = gaze_look_offset(elapsed / GAZE_LOOK_PERIOD + shift.0, size);
    let eyes = if gaze_blink_closed(elapsed / GAZE_BLINK_PERIOD + shift.1) {
        &faces.eyes_closed
    } else {
        &faces.eyes
    };
    div()
        .size(px(size))
        .overflow_hidden()
        .child(
            gpui::img(faces.body.clone())
                .size(px(size))
                .rounded(px(6.0)),
        )
        .child(
            div()
                .absolute()
                .left(px(dx))
                .top(px(dy))
                .size(px(size))
                .child(gpui::img(eyes.clone()).size(px(size)).rounded(px(6.0))),
        )
        .into_any_element()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BossTab {
    Memory,
    Personas,
    Employees,
    Plans,
    Deliverables,
}

/// One original note in the unified Memory Records feed — the daemon's
/// `Feed` record carries the note's stable identity and text plus its
/// provenance: bucket label and resolved project association. Summaries
/// never appear; the feed lists originals only.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemoryFeedRecord {
    /// The note's bucket-local stable id — `"{sequence}-{digest}"`.
    id: String,
    bucket_id: String,
    /// The owning bucket's display name — a provenance label, not a filter.
    bucket: String,
    sequence: u64,
    text: String,
    created_at: u64,
    /// `None` — the bucket is confirmed to have no project association, the
    /// feed's "No project" grouping.
    project: Option<MemoryFeedProject>,
}

/// The record's repository association, keyed on the project bucket id so
/// every worktree of one repository collapses to a single project choice.
/// `name` resolves through the project catalog — `None` when nothing does,
/// the feed's "Unknown project" grouping.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemoryFeedProject {
    key: String,
    name: Option<String>,
}

impl MemoryFeedRecord {
    /// The row's stable identity — note ids are only unique inside their
    /// bucket.
    fn key(&self) -> String {
        format!("{}/{}", self.bucket_id, self.id)
    }
}

/// The Memory section's project choice — `All` is the default. The rest
/// match the plan's association groups; `Bucket` keys on the repository's
/// shared bucket id so one repository's worktrees stay a single choice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum MemoryProjectFilter {
    #[default]
    All,
    /// Records in buckets confirmed to have no project association.
    NoProject,
    /// Records in project buckets no catalog project resolves to.
    Unknown,
    /// One repository's shared bucket — its resolved project label shows.
    Bucket(String),
}

/// How many filtered records a fresh feed view reveals — the "show more"
/// footer pages the rest in this many at a time.
const MEMORY_FEED_PAGE: usize = 50;

/// Text longer than this — or spanning more lines than the clamp shows —
/// gets the accessible expansion control rather than rendering in full.
const MEMORY_EXPAND_CHARS: usize = 480;
const MEMORY_EXPAND_LINES: usize = 6;

/// One host's unified Memory Records feed — the whole accessible corpus
/// plus its browse state. `records` keep the daemon's newest-first order;
/// filtering and paging never reorder them.
struct MemoryFeed {
    records: Vec<MemoryFeedRecord>,
    /// A response has landed — distinguishes "loading" from "failed before
    /// anything arrived".
    loaded: bool,
    /// A request is in flight or parked behind another boss operation.
    loading: bool,
    /// The last failure — records already on screen stay beside it.
    error: Option<String>,
    /// How many filtered rows the list reveals — "show more" grows it.
    visible: usize,
    /// Record keys whose full text is expanded.
    expanded: HashSet<String>,
    /// A refresh that found only newer records, held until the user
    /// activates "New memories available" — applying it directly would
    /// shift whatever row is being read.
    pending_refresh: Vec<MemoryFeedRecord>,
    /// The project filter cleared itself because its choice left the feed —
    /// surfaced until the next browse action.
    filter_notice: Option<String>,
}

impl Default for MemoryFeed {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            loaded: false,
            loading: false,
            error: None,
            visible: MEMORY_FEED_PAGE,
            expanded: HashSet::new(),
            pending_refresh: Vec::new(),
            filter_notice: None,
        }
    }
}

/// Whether a record survives the feed's combined search and project
/// filtering — the trimmed lowercase query matches text, bucket label, and
/// resolved project name.
fn memory_record_matches(
    record: &MemoryFeedRecord,
    query: &str,
    filter: &MemoryProjectFilter,
) -> bool {
    let in_project = match filter {
        MemoryProjectFilter::All => true,
        MemoryProjectFilter::NoProject => record.project.is_none(),
        MemoryProjectFilter::Unknown => record
            .project
            .as_ref()
            .is_some_and(|project| project.name.is_none()),
        MemoryProjectFilter::Bucket(key) => record
            .project
            .as_ref()
            .is_some_and(|project| project.key == *key),
    };
    if !in_project || query.is_empty() {
        return in_project;
    }
    record.text.to_lowercase().contains(query)
        || record.bucket.to_lowercase().contains(query)
        || record
            .project
            .as_ref()
            .and_then(|project| project.name.as_deref())
            .is_some_and(|name| name.to_lowercase().contains(query))
}

/// The project filter's choices — alphabetical resolved projects first,
/// then the two pseudo-entries, each offered only while records represent
/// it. `All projects` itself is rendered by the caller.
fn memory_project_options(records: &[MemoryFeedRecord]) -> Vec<(MemoryProjectFilter, String)> {
    let mut options: Vec<(MemoryProjectFilter, String)> = Vec::new();
    let mut has_none = false;
    let mut has_unknown = false;
    for record in records {
        match &record.project {
            None => has_none = true,
            Some(project) if project.name.is_none() => has_unknown = true,
            Some(project) => {
                let name = project.name.clone().unwrap_or_default();
                if !options.iter().any(|(filter, _)| {
                    matches!(filter, MemoryProjectFilter::Bucket(key) if *key == project.key)
                }) {
                    options.push((MemoryProjectFilter::Bucket(project.key.clone()), name));
                }
            }
        }
    }
    options.sort_by(|(_, a), (_, b)| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    if has_none {
        options.push((MemoryProjectFilter::NoProject, tr!("project.no_project_name")));
    }
    if has_unknown {
        options.push((MemoryProjectFilter::Unknown, tr!("sidebar.unknown_project")));
    }
    options
}

/// The feed row's compact creation age — `now`, `5m`, `2h`, `3d`.
fn memory_record_age(seconds: u64) -> String {
    match seconds {
        0..=59 => tr!("boss.memory_now"),
        60..=3_599 => tr!("sidebar.minutes_ago", count = seconds / 60),
        3_600..=86_399 => tr!("sidebar.hours_ago", count = seconds / 3_600),
        _ => tr!("sidebar.days_ago", count = seconds / 86_400),
    }
}

/// The feed row's exact creation time for the timestamp tooltip.
fn memory_record_exact_time(at: u64) -> String {
    chrono::DateTime::from_timestamp(at as i64, 0)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%b %-d, %Y, %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The Employees section's roster views — active work is running and
/// queued together; history is the finished roster plus retired records.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum BossEmployeesView {
    #[default]
    Active,
    History,
}

/// The History lookup's date filter presets — bounds resolve at request
/// time so "last 30 days" means what it says when the search runs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum HistoryDateRange {
    #[default]
    Any,
    Last7,
    Last30,
    Last90,
    Before90,
}

impl HistoryDateRange {
    const DAY: u64 = 86_400;

    /// `(after, before)` message-date bounds for the daemon, in unix
    /// seconds.
    fn bounds(self, now: u64) -> (Option<u64>, Option<u64>) {
        match self {
            Self::Any => (None, None),
            Self::Last7 => (Some(now.saturating_sub(7 * Self::DAY)), None),
            Self::Last30 => (Some(now.saturating_sub(30 * Self::DAY)), None),
            Self::Last90 => (Some(now.saturating_sub(90 * Self::DAY)), None),
            Self::Before90 => (None, Some(now.saturating_sub(90 * Self::DAY))),
        }
    }
}

/// One host's Employees → History lookup — the filter picks, accumulated
/// hits, and the coverage the last reply reported. Kept per host so
/// leaving a source's transcript and returning restores the same list
/// under the same query.
#[derive(Default)]
pub(super) struct BossHistorySearch {
    /// Project filter for the next request — the daemon-side project id.
    pub project: Option<Uuid>,
    /// Source-kind filter for the next request.
    pub kind: Option<waku_protocol::model::HistorySourceKind>,
    /// Date bounds for the next request.
    pub range: HistoryDateRange,
    /// Hits so far — a fresh lookup replaces them, a continuation appends.
    pub hits: Vec<waku_protocol::model::AgentHistorySearchHit>,
    /// What the last reply scanned and capped — `None` until one lands.
    pub coverage: Option<waku_protocol::model::AgentHistorySearchCoverage>,
    /// A request is in flight or parked for the current selections.
    pub searching: bool,
    /// The in-flight request's page offset — `0` replaces the hits, a
    /// "show more" continuation appends to them.
    pub pending_offset: Option<usize>,
    /// The last failure — prior hits stay on screen beside it.
    pub error: Option<String>,
}

/// Whether the lookup owns the list — field text or any filter pick
/// engages it; empty-on-defaults keeps the finished-roster list.
fn boss_history_search_active(query: &str, search: Option<&BossHistorySearch>) -> bool {
    !query.trim().is_empty()
        || search.is_some_and(|search| {
            search.project.is_some()
                || search.kind.is_some()
                || search.range != HistoryDateRange::Any
        })
}

/// The lookup's list rows — one per accumulated hit, then the coverage
/// footer while the search has state worth reporting.
fn boss_history_rows(search: &BossHistorySearch) -> Vec<BossItem> {
    let mut rows: Vec<BossItem> = (0..search.hits.len()).map(BossItem::HistoryHit).collect();
    if search.searching || search.error.is_some() || search.coverage.is_some() {
        rows.push(BossItem::HistoryFooter);
    }
    rows
}

/// Append a continuation page to the accumulated hits. A record re-ranked
/// between pages can repeat — dedupe on the excerpted message.
fn merge_history_page(
    hits: &mut Vec<waku_protocol::model::AgentHistorySearchHit>,
    page: Vec<waku_protocol::model::AgentHistorySearchHit>,
) {
    let seen: HashSet<(Uuid, Uuid)> = hits
        .iter()
        .map(|hit| (hit.task_id, hit.message_id))
        .collect();
    hits.extend(
        page.into_iter()
            .filter(|hit| !seen.contains(&(hit.task_id, hit.message_id))),
    );
}

/// One hit row's element id — task plus excerpted message keeps entries
/// distinct when two passages land from the same record.
fn boss_history_hit_id(task_id: Uuid, message_id: Uuid) -> SharedString {
    SharedString::from(format!("boss-history-hit-{task_id}-{message_id}"))
}

/// The Plans section's document filter. Approved plans keep their record
/// after the planning session archives; an archived session on an
/// unfinished draft is what Archived means here.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum BossPlansFilter {
    #[default]
    Active,
    Approved,
    Archived,
}

/// The Deliverables library's record filter — the sidebar's recency
/// window does not apply; every published record lists under one of
/// these.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum BossDeliverablesFilter {
    /// Live rows — everything the record does not hide: dormant and
    /// archived records list under their own filters.
    #[default]
    All,
    Pinned,
    Dormant,
    Archived,
}

pub(super) struct BossUi {
    pub states: HashMap<DaemonKey, BossState>,
    pub chat_history: HashMap<DaemonKey, super::boss_history::BossChatHistory>,
    /// Scoped to building one archived row; composer and callbacks keep the live owner.
    pub history_render_session: Cell<Option<Uuid>>,
    pub(super) outcome_rows: HashMap<DaemonKey, Arc<Vec<BossOutcomeRow>>>,
    /// The Goals panel's two scroll regions: a bounded Finished history on
    /// top and the In progress/Pending sections below. Fold and history
    /// choices are honored per Boss daemon across snapshots.
    pub(super) goals_finished_list: ListState,
    pub(super) goals_finished_scrollbar: Rc<ScrollbarState>,
    pub(super) goals_ongoing_list: ListState,
    pub(super) goals_ongoing_scrollbar: Rc<ScrollbarState>,
    pub(super) goals_list_owner: Option<DaemonKey>,
    pub(super) goals_finished_signature: Option<u64>,
    pub(super) goals_ongoing_signature: Option<u64>,
    pub(super) goals_collapsed: HashSet<(DaemonKey, BossGoalSection)>,
    pub(super) goals_history_expanded: HashSet<DaemonKey>,
    /// Outcome rows the user opened for assignment detail — honored per
    /// Boss daemon across snapshots and section moves.
    pub(super) goals_row_expanded: HashSet<(DaemonKey, Uuid)>,
    /// A stable focus handle per focusable Goals item — outcome row,
    /// expanded attempt entry, or section header — keyed by each item's
    /// stable id. A row changing sections remounts between the two
    /// virtualized lists; sharing one handle keeps `Window`'s focus on
    /// the same element across the move, and seeding it into the list's
    /// item records keeps a focused item rendering while it sits outside
    /// the painted range. Handles are pruned as items leave the panel.
    pub(super) goals_focus_handles: HashMap<(DaemonKey, String), FocusHandle>,
    pub projects: HashMap<Uuid, Project>,
    pub hosts: Vec<DaemonKey>,
    pub managed: HashSet<Uuid>,
    /// Rotated Boss chats: retired session id → replacement session id.
    /// Composer draft keys resolve through this so a swap mid-draft re-keys
    /// the slot instead of stranding the text on the archived row.
    pub(super) rotated_chat_drafts: HashMap<Uuid, Uuid>,
    pub identities: HashMap<Uuid, BossIdentity>,
    /// Cached `ListBuckets` replies per daemon — the `@` mention pool's
    /// memory-bucket source. Fetched lazily the first time a boss surface's
    /// pool is drawn; a daemon that never answers simply lists none.
    pub(super) buckets: HashMap<DaemonKey, Rc<Vec<BossBucketRef>>>,
    /// Daemons a bucket pull is already in flight for — interior-mutable so
    /// the render-path pool draw can arm a fetch without `&mut self`.
    pub(super) buckets_requested: RefCell<HashSet<DaemonKey>>,
    /// The `memory/` tree listing per daemon — the `@` mention pool's
    /// memory-file source, fetched with the same lazy one-shot rule as
    /// `buckets`.
    pub(super) memory_files: HashMap<DaemonKey, Rc<Vec<BossFile>>>,
    /// Daemons a memory-tree pull is already in flight for.
    pub(super) memory_files_requested: RefCell<HashSet<DaemonKey>>,
    pub(super) job_titles: HashMap<Uuid, String>,
    pub(super) employee_icons: HashMap<Uuid, Option<CustomCommandIcon>>,
    pub active: HashMap<DaemonKey, Vec<Uuid>>,
    pub working: HashSet<Uuid>,
    pub expired: HashSet<Uuid>,
    workspace_transitions: HashSet<Uuid>,
    /// Queued employees' wait reason, keyed by session id — the sidebar row
    /// and summon card read membership here to wear a pending state instead
    /// of the finished/idle look an unstarted shell session would give them.
    pub(super) queued: HashMap<Uuid, String>,
    /// Employees mid-dispatch: the slot is granted but the provider has not
    /// started, so the shell can still read Idle — the row shows a starting
    /// spinner until the Working lifecycle lands.
    pub(super) dispatching: HashSet<Uuid>,
    /// Resolved ticket target for queued and dispatching employees, keyed by
    /// session id so sidebar and summon rows can reveal it without rescanning
    /// every Boss snapshot while rendering.
    pub(super) queued_model_targets:
        HashMap<Uuid, (DaemonKey, ProviderKind, String, Option<String>)>,
    pub recent: HashMap<DaemonKey, Vec<Uuid>>,
    pub sidebar_idle_visible: HashMap<DaemonKey, usize>,
    /// The deliverable a sidebar click armed the composer with: the next main-
    /// composer submission commands the boss with this file attached.
    /// Cleared by a send or any landing that is not re-arming it — the
    /// `pending_deliverable` half carries it across its own navigation.
    pub command_deliverable: Option<(DaemonKey, Uuid)>,
    /// Memory record armed as context for a user-requested correction in
    /// Boss chat — `(key, label, content)` where the label is a
    /// `buckets/<name>/note-<seq>` record reference.
    pub(super) command_memory_correction: Option<(DaemonKey, String, String)>,
    /// The deliverable whose row click is still navigating to its task page —
    /// the boss chat. Session activation clears `command_deliverable` as stale
    /// context, so the click parks its deliverable here and the finish reapplies
    /// the arm once the boss chat is on screen. The flag is whether the
    /// landing should write a history entry: a fresh row click pushes the
    /// page from its original surface; a back/forward hop restoring it
    /// must not re-push. The final field retains that original surface
    /// across activation of the boss chat underneath the preview.
    pub pending_deliverable: Option<(DaemonKey, Uuid, bool, Option<NavigationLocation>)>,
    /// The deliverable whose preview page covers the boss chat's transcript: a
    /// previewable file's own page rather than a panel off the chat. Lives
    /// and dies with `command_deliverable` — the same arm brands the composer's
    /// boss chip and roots the page's file at the deliverable's directory.
    pub deliverable_page: Option<(DaemonKey, Uuid)>,
    /// Markdown list offsets for preview pages, scoped to the owning daemon
    /// and deliverable so opening another file cannot inherit this position.
    pub(super) deliverable_page_scroll: HashMap<(DaemonKey, Uuid), ListState>,
    pub revision: u64,
    pub page: Option<(DaemonKey, BossTab)>,
    last_section: HashMap<DaemonKey, BossTab>,
    /// The Plans tab's selected plan — a `BossState.planning` record's
    /// session id, resolved against the live state at render. Kept per
    /// host so a second daemon's page restores its own selection.
    plans_selected: HashMap<DaemonKey, Uuid>,
    /// The Personas section's selected role, per host — the boss's own
    /// persona id is a valid entry like any role's.
    personas_selected: HashMap<DaemonKey, Uuid>,
    /// Per-section filters, per host: the Employees view and the Plans and
    /// Deliverables library filters.
    employees_view: HashMap<DaemonKey, BossEmployeesView>,
    plans_filter: HashMap<DaemonKey, BossPlansFilter>,
    deliverables_filter: HashMap<DaemonKey, BossDeliverablesFilter>,
    /// The Personas list's name query — mirrored off the search input so
    /// `sync_boss_page_rows` can filter without an `App` context.
    persona_query: String,
    persona_search: Option<Entity<TextInput>>,
    /// Read-only persona instructions — the detail pane's markdown cache,
    /// selection, and scroll position, keyed to the selected persona.
    persona_markdown: RefCell<Option<(Uuid, MarkdownView)>>,
    /// The read-only inherited Employee base shown under a custom role's
    /// detail — a second cache because the role's own text owns
    /// `persona_markdown`.
    persona_base_markdown: RefCell<Option<(Uuid, MarkdownView)>>,
    persona_selection: TranscriptSelection,
    persona_scroll: ScrollHandle,
    persona_scrollbar: Rc<ScrollbarState>,
    /// Per-persona expansion for the shipped-default card — comparison,
    /// shipped text, and the reset confirmation, keyed by persona id.
    persona_default_pane: HashMap<Uuid, PersonaDefaultPane>,
    /// The shipped↔saved diff text for an expanded comparison, keyed to
    /// the markdown fingerprint it was computed from.
    persona_diff: RefCell<Option<(Uuid, u64, String)>>,
    /// The unified Memory Records feed per host — records, browse state,
    /// and the pending refresh a visit re-checks for.
    memory_feed: HashMap<DaemonKey, MemoryFeed>,
    /// The feed's project choice per host — absent means All projects.
    memory_project: HashMap<DaemonKey, MemoryProjectFilter>,
    /// The `boss_memory_search` field's current text, mirrored on Edited so
    /// row building never touches the input entity.
    pub(super) memory_query: String,
    /// A `Feed` operation parked while another request held the pipe — a
    /// refresh issued mid-flight replaces it rather than dropping. Last
    /// submission wins.
    queued_memory_feed: Option<(DaemonKey, BossOperation)>,
    editor: Option<BossEditor>,
    generation: u64,
    pending: bool,
    pending_reply: Option<BossReply>,
    /// Planning sessions whose `finalizePlan` dispatch went out and did
    /// not come back failed — the approval chip hides and the composer
    /// seals for the in-flight window. Success leaves the entry until
    /// `finalized_at` lands on the session so neither flickers back; a
    /// failed reply removes it so both restore for a retry.
    pub(super) plan_finalizing: HashSet<Uuid>,
    /// An `Open` a boss-chat click carried while another request held the
    /// pipe. Navigation activates the locally held session immediately and
    /// parks the operation here; the next request completion re-issues it so
    /// the daemon still sees the open. Last click wins.
    queued_open: Option<(DaemonKey, BossOperation)>,
    /// A `HistorySearch` parked while another request held the pipe — a
    /// refinement submitted mid-flight replaces the park rather than
    /// dropping silently. Last submission wins.
    queued_history_search: Option<(DaemonKey, BossOperation)>,
    /// The Employees → History lookup state per host — filters, hits,
    /// coverage, and the last error. Absent means never searched.
    pub(super) history_search: HashMap<DaemonKey, BossHistorySearch>,
    /// The `boss_history_search` field's current text, mirrored on Edited
    /// so row building never touches the input entity.
    pub(super) history_query: String,
    list: ListState,
    scrollbar: Rc<ScrollbarState>,
    rows: Vec<BossItem>,
    avatar_queue: RefCell<VecDeque<(String, AvatarStyle, u32, u8)>>,
    // Marks keys queued or in flight so a render pass cannot enqueue
    // duplicates; exhausted failures stay marked to bound retry churn.
    // Eviction clears the mark — a face on screen may be asked for again.
    avatar_requested: RefCell<HashSet<((String, AvatarStyle), u32)>>,
    avatars: HashMap<(String, AvatarStyle), HashMap<u32, AvatarFaces>>,
    avatar_active: usize,
    focus: Option<FocusHandle>,
}

impl Default for BossUi {
    fn default() -> Self {
        Self {
            states: HashMap::new(),
            chat_history: HashMap::new(),
            history_render_session: Cell::new(None),
            outcome_rows: HashMap::new(),
            goals_finished_list: ListState::new(0, ListAlignment::Top, px(240.0)),
            goals_finished_scrollbar: ScrollbarState::new(),
            goals_ongoing_list: ListState::new(0, ListAlignment::Top, px(640.0)),
            goals_ongoing_scrollbar: ScrollbarState::new(),
            goals_list_owner: None,
            goals_finished_signature: None,
            goals_ongoing_signature: None,
            goals_collapsed: HashSet::new(),
            goals_history_expanded: HashSet::new(),
            goals_row_expanded: HashSet::new(),
            goals_focus_handles: HashMap::new(),
            projects: HashMap::new(),
            hosts: Vec::new(),
            managed: HashSet::new(),
            rotated_chat_drafts: HashMap::new(),
            identities: HashMap::new(),
            buckets: HashMap::new(),
            buckets_requested: RefCell::new(HashSet::new()),
            memory_files: HashMap::new(),
            memory_files_requested: RefCell::new(HashSet::new()),
            job_titles: HashMap::new(),
            employee_icons: HashMap::new(),
            active: HashMap::new(),
            working: HashSet::new(),
            expired: HashSet::new(),
            workspace_transitions: HashSet::new(),
            queued: HashMap::new(),
            dispatching: HashSet::new(),
            queued_model_targets: HashMap::new(),
            recent: HashMap::new(),
            sidebar_idle_visible: HashMap::new(),
            command_deliverable: None,
            command_memory_correction: None,
            pending_deliverable: None,
            deliverable_page: None,
            deliverable_page_scroll: HashMap::new(),
            revision: 0,
            page: None,
            last_section: HashMap::new(),
            plans_selected: HashMap::new(),
            personas_selected: HashMap::new(),
            employees_view: HashMap::new(),
            plans_filter: HashMap::new(),
            deliverables_filter: HashMap::new(),
            persona_query: String::new(),
            persona_search: None,
            persona_markdown: RefCell::new(None),
            persona_base_markdown: RefCell::new(None),
            persona_selection: TranscriptSelection::default(),
            persona_scroll: ScrollHandle::new(),
            persona_scrollbar: ScrollbarState::new(),
            persona_default_pane: HashMap::new(),
            persona_diff: RefCell::new(None),
            memory_feed: HashMap::new(),
            memory_project: HashMap::new(),
            memory_query: String::new(),
            queued_memory_feed: None,
            editor: None,
            generation: 0,
            pending: false,
            pending_reply: None,
            plan_finalizing: HashSet::new(),
            queued_open: None,
            queued_history_search: None,
            history_search: HashMap::new(),
            history_query: String::new(),
            list: ListState::new(0, ListAlignment::Top, px(640.0)),
            scrollbar: ScrollbarState::new(),
            rows: Vec::new(),
            avatar_queue: RefCell::new(VecDeque::new()),
            avatar_requested: RefCell::new(HashSet::new()),
            avatars: HashMap::new(),
            avatar_active: 0,
            focus: None,
        }
    }
}

impl BossUi {
    fn retry_avatar(&self, seed: (String, AvatarStyle), bucket: u32, attempt: u8) {
        if attempt < AVATAR_MAX_ATTEMPTS {
            self.avatar_queue
                .borrow_mut()
                .push_back((seed.0, seed.1, bucket, attempt + 1));
        }
    }

    fn cache_avatar(
        &mut self,
        seed: (String, AvatarStyle),
        bucket: u32,
        faces: AvatarFaces,
    ) -> Option<AvatarFaces> {
        let cached: usize = self.avatars.values().map(|buckets| buckets.len()).sum();
        let evicted = if cached >= AVATAR_CACHE_LIMIT {
            let key = self.avatars.iter().find_map(|(seed, buckets)| {
                buckets.keys().next().map(|bucket| (seed.clone(), *bucket))
            });
            key.and_then(|(old_seed, old_bucket)| {
                let buckets = self.avatars.get_mut(&old_seed)?;
                let image = buckets.remove(&old_bucket);
                if buckets.is_empty() {
                    self.avatars.remove(&old_seed);
                }
                // Release the evicted key's dedup mark so the next request
                // requeues it. Only on-screen surfaces enqueue renders, so
                // the churn the mark used to prevent — mention preparation
                // requeueing the whole roster — cannot recur.
                self.avatar_requested
                    .borrow_mut()
                    .remove(&(old_seed, old_bucket));
                image
            })
        } else {
            None
        };
        self.avatars.entry(seed).or_default().insert(bucket, faces);
        evicted
    }

    /// An unread deliverable opens at the top of its page — the offset a
    /// previous open left belongs to the version that was read, and a
    /// re-publish since then moves the content that position pointed at.
    /// A deliverable still read keeps its place across remounts.
    fn forget_unread_deliverable_scroll(&mut self, key: DaemonKey, deliverable_id: Uuid) {
        let unread = self
            .states
            .get(&key)
            .and_then(|state| {
                state
                    .deliverables
                    .iter()
                    .find(|deliverable| deliverable.id == deliverable_id)
            })
            .is_some_and(|deliverable| {
                deliverable
                    .viewed_at
                    .is_none_or(|viewed| viewed < deliverable.updated_at)
            });
        if unread {
            self.deliverable_page_scroll.remove(&(key, deliverable_id));
        }
    }
}

/// One memory bucket the `@` mention pool can name — the fields a
/// `ListBuckets` reply carries, parsed at fetch so rows never read the
/// daemon's loose JSON.
#[derive(Clone)]
pub(super) struct BossBucketRef {
    pub id: String,
    pub name: String,
    pub purpose: String,
}

/// The `ListBuckets` reply's per-bucket shape — the daemon serializes the
/// memory engine's record as loose JSON, so the client reads only the
/// fields a mention needs.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireBossBucket {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    purpose: String,
}

/// A Goals panel section, in display order. Only `EmployeeGoal::Goal`
/// records reach these lists — errands never appear in the tab.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum BossGoalSection {
    Finished,
    InProgress,
    Pending,
}

/// One daemon-owned outcome prepared for the Tasks panel — the durable
/// record plus the roster members linked to it. The panel lists outcomes
/// only: an assignment without `Assignment::outcome_id` produces no row,
/// and no synthetic outcome is created to give unlinked work a row.
#[derive(Clone)]
pub(super) struct BossOutcomeRow {
    /// The durable record — outcome text, stored state, recorded waits,
    /// handoffs, and the chronological assignment history.
    pub outcome: waku_protocol::boss::BossOutcome,
    /// Members linked by `Assignment::outcome_id` — live roster records
    /// first, then retired ones, each in roster order. Live records drive
    /// the status join; expired and retired ones still classify their
    /// settled attempts.
    pub members: Vec<BossOutcomeMember>,
    /// Attention derived across both rosters and the durable history —
    /// an unresolved failure, a flagged blocker, or an open completion
    /// conflict, including a settled attempt whose roster record is gone.
    pub attention: bool,
}

/// An outcome member's roster record plus its queue position. The record
/// rides along so the render pass reads lifecycle, ticket waits, and
/// settle evidence without rescanning the roster.
#[derive(Clone)]
pub(super) struct BossOutcomeMember {
    pub employee: waku_protocol::boss::BossEmployee,
    /// 1-based admission order among the daemon's queued employees —
    /// `None` once dispatched or for members that never queued.
    pub queue_rank: Option<usize>,
}

/// An outcome's attention flag — the roster's unresolved members and any
/// open completion conflict, extended to settled history rows whose
/// roster record is gone so a failure cannot lose attention when the
/// employee retires. Pending handoffs are owed decisions, not attention:
/// the row surfaces them through its own follow-up state.
fn boss_outcome_attention(
    outcome: &waku_protocol::boss::BossOutcome,
    members: &[BossOutcomeMember],
) -> bool {
    if outcome.completion_conflict.is_some()
        || members.iter().any(|member| {
            waku_protocol::boss::BossOutcome::assignment_unresolved(&member.employee)
        })
    {
        return true;
    }
    // A session's latest recorded attempt still failed or blocked while
    // its roster record is gone — the durable row is all that remembers
    // the failure. A rostered member defers to the record's own verdict:
    // a resume reopens its row, so a settled failure here is one nothing
    // answered.
    let mut latest: HashMap<Uuid, &waku_protocol::boss::OutcomeAssignment> = HashMap::new();
    for row in &outcome.assignments {
        latest.insert(row.session, row);
    }
    latest.values().any(|row| {
        row.settled.as_ref().is_some_and(|settle| {
            matches!(
                settle.verdict,
                waku_protocol::boss::AssignmentVerdict::Failed
            ) || settle.blocked
        }) && !members
            .iter()
            .any(|member| member.employee.session_id == row.session)
    })
}

/// The Deliverables library rows under `filter`. A planning session's
/// published plan document (`plan_id` set) stays inside its planning
/// flow — the Plans tab and the session row — so it never lists here
/// under any filter, just as the sidebar group skips it.
fn boss_deliverable_rows(state: &BossState, filter: BossDeliverablesFilter) -> Vec<BossItem> {
    let mut deliverables: Vec<&waku_protocol::boss::BossDeliverable> = state
        .deliverables
        .iter()
        .filter(|deliverable| deliverable.plan_id.is_none())
        .filter(|deliverable| {
            let pinned = deliverable.pinned_at.is_some();
            let dormant = deliverable.dormant_at.is_some() && !pinned;
            let archived = deliverable.archived_at.is_some();
            match filter {
                BossDeliverablesFilter::All => !dormant && !archived,
                BossDeliverablesFilter::Pinned => pinned && !archived,
                BossDeliverablesFilter::Dormant => dormant && !archived,
                BossDeliverablesFilter::Archived => archived,
            }
        })
        .collect();
    // Pinned records lead the live list the way they lead the
    // sidebar group; every filter then sorts by last publish.
    deliverables.sort_by_key(|deliverable| {
        (
            !deliverable.pinned_at.is_some(),
            std::cmp::Reverse(deliverable.updated_at),
        )
    });
    deliverables
        .into_iter()
        .map(|deliverable| BossItem::Deliverable(deliverable.id))
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BossItem {
    Employee(Uuid),
    Persona(Uuid),
    /// A `BossState.planning` record's session — Plans tab rows.
    Plan(Uuid),
    /// A `BossState.deliverables` record — the library rows.
    Deliverable(Uuid),
    /// A History lookup hit — the index into the host's accumulated
    /// `BossHistorySearch::hits`.
    HistoryHit(usize),
    /// The History lookup's coverage row — scope, caveats, and the
    /// continuation affordance, drawn after the hits.
    HistoryFooter,
    /// A unified Memory Records row — the index into the host feed's
    /// `records`, stable across search and project filtering.
    MemoryRecord(usize),
    /// The feed's "show more" affordance, drawn after the revealed records
    /// with the filtered count still hidden behind it.
    MemoryFooter { remaining: usize },
}

/// An armed composer command: the boss chat that answers the next
/// submission and the context it attaches.
pub(super) struct BossCommand {
    /// The boss's own chat session — where the submission lands.
    pub session_id: Uuid,
    /// The boss's identity — the composer's destination chip.
    pub identity: BossIdentity,
    pub context: BossCommandContext,
}

/// What a boss-command submission attaches: the live employee whose task
/// is on screen, or the deliverable a sidebar click armed.
pub(super) enum BossCommandContext {
    Employee(Uuid),
    /// A read-only memory record the user is asking the Boss to correct —
    /// `path` is the record's `buckets/<name>/note-<seq>` label; `content`
    /// is the shown text.
    MemoryCorrection {
        path: String,
        content: String,
    },
    Deliverable {
        path: PathBuf,
        name: String,
        directory: bool,
    },
}

/// The persona form behind the detail pane's deliberate Edit mode —
/// `persona` is `Uuid::nil()` while a new role is being drafted.
struct BossEditor {
    key: DaemonKey,
    persona: Uuid,
    name: Entity<TextInput>,
    content: Entity<TextInput>,
    pinned: Entity<TextInput>,
    buckets: Entity<TextInput>,
    permissions: PersonaPermissions,
    icon: Option<CustomCommandIcon>,
    original_icon: Option<CustomCommandIcon>,
    original: Vec<String>,
    original_permissions: PersonaPermissions,
    integrations: Vec<String>,
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum BossReply {
    Open,
    List,
    /// The unified `Feed` page for the Memory section's record list.
    MemoryFeed,
    Saved,
    /// A `finalizePlan` dispatch for the planning session it names — the
    /// id marks the press's in-flight window so the chip hides and the
    /// composer seals until the reply lands.
    Finalize(Uuid),
    /// A shipped-default action — reset, undo, keep, or a proposal
    /// decision. The result refreshes state like `Saved` without the
    /// editor bookkeeping.
    PersonaDefault,
    /// The human's pick of the canonical Employee base — reports its own
    /// toast rather than the instruction-update one.
    BaseChoice,
    /// A manual `resume` dispatch — confirms the re-admission and pulls
    /// the fresh roster rather than reusing `Saved`'s editor bookkeeping.
    Resume,
    /// An Employees → History lookup — the reply merges into the host's
    /// `BossHistorySearch` instead of the roster list.
    HistorySearch,
}

/// Expansion state for one canonical default's instructions card.
#[derive(Clone, Copy, Default)]
struct PersonaDefaultPane {
    /// The saved↔shipped unified diff is on screen.
    compare: bool,
    /// The full latest shipped instructions are on screen.
    shipped: bool,
    /// The reset confirmation preview is on screen.
    reset_preview: bool,
}

/// Sidebar employee order: newest summon first. `created_at` is the
/// summon stamp — records predating the field fall back to their
/// session's creation time, also stamped at summon. Sorting on anything
/// mutable, like session activity, would shuffle a row on every state
/// revision; this key never changes after the employee appears.
fn sort_boss_employees(
    employees: &mut Vec<&waku_protocol::boss::BossEmployee>,
    session_created_at: &HashMap<Uuid, u64>,
) {
    employees.sort_by_key(|employee| {
        std::cmp::Reverse(
            employee
                .created_at
                .or_else(|| session_created_at.get(&employee.session_id).copied())
                .unwrap_or(0),
        )
    });
}

/// The icon an employee's rows show: the summon-set icon first, then its
/// persona's — restricted to the employee icon set either way.
fn employee_icon(
    employee: &waku_protocol::boss::BossEmployee,
    state: &BossState,
) -> Option<CustomCommandIcon> {
    employee
        .icon
        .filter(|icon| icon.is_employee_icon())
        .or_else(|| {
            state
                .personas
                .iter()
                .find(|persona| persona.id == employee.persona_id)
                .and_then(|persona| persona.icon)
                .filter(|icon| icon.is_employee_icon())
        })
}

/// `Waku::session_is_employee` without the self: an employee is managed by
/// a boss — stamped `boss_managed` at summon or still in the live `managed`
/// roster — while the boss's own surfaces (`boss_chat`, planning sessions)
/// are not employees.
fn employee_session(
    session: &AgentSession,
    managed: &HashSet<Uuid>,
    boss_chat: bool,
    experiment_enabled: bool,
) -> bool {
    experiment_enabled
        && (session.boss_managed || managed.contains(&session.id))
        && !boss_chat
        && !session.is_planning()
}

/// An archived employee session, read off durable state: `archived_at`
/// from the archive and `boss_managed` from the summon both survive the
/// employee's roster retirement, which is how history and search still
/// reach the task after the row is gone.
fn archived_employee_session(
    session: &AgentSession,
    managed: &HashSet<Uuid>,
    boss_chat: bool,
    experiment_enabled: bool,
) -> bool {
    session.archived_at.is_some()
        && employee_session(session, managed, boss_chat, experiment_enabled)
}

/// The Boss state owning one of the boss's own surfaces — its chat's
/// `session_id` or a `planning` record's. Employee sessions resolve
/// through `boss_ui.identities` instead, so this answers only for the
/// boss itself.
fn boss_state_owning_session(
    states: &HashMap<DaemonKey, BossState>,
    session_id: Uuid,
) -> Option<&BossState> {
    states.values().find(|state| {
        state.session_id == Some(session_id)
            || state
                .planning
                .iter()
                .any(|plan| plan.session_id == session_id)
    })
}

fn viewed_plan_just_finalized(
    previous: Option<&BossState>,
    current: &BossState,
    selected_session: Option<Uuid>,
) -> bool {
    let Some(previous) = previous.filter(|previous| previous.identity.id == current.identity.id)
    else {
        return false;
    };
    let Some(session_id) = selected_session else {
        return false;
    };
    previous
        .planning
        .iter()
        .any(|plan| plan.session_id == session_id && plan.finalized_at.is_none())
        && current
            .planning
            .iter()
            .any(|plan| plan.session_id == session_id && plan.finalized_at.is_some())
}

impl Waku {
    pub(super) fn drain_boss_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        let mut finalized_plan_host = None;
        let mut rotated_chat_host = None;
        while let Ok((key, state)) = self.boss_events.try_recv() {
            if self.boss_ui.states.get(&key).is_some_and(|previous| {
                previous.identity.id == state.identity.id && previous.revision > state.revision
            }) {
                continue;
            }
            // Agent-initiated approval has no BossReply in this client.
            // Follow its live state transition, but don't redirect when
            // opening an already-finalized plan or syncing another host.
            if viewed_plan_just_finalized(
                self.boss_ui.states.get(&key),
                &state,
                self.state.selected_session,
            ) && !self
                .state
                .selected_session
                .is_some_and(|id| self.boss_ui.plan_finalizing.contains(&id))
            {
                finalized_plan_host = Some(key);
            }
            if self.boss_ui.page.is_none()
                && self.boss_ui.states.get(&key).is_some_and(|previous| {
                    previous.identity.id == state.identity.id
                        && previous.session_id.is_some()
                        && previous.session_id == self.state.selected_session
                        && state.session_id.is_some()
                        && previous.session_id != state.session_id
                })
            {
                rotated_chat_host = Some(key);
            }
            // Rotation re-keys the boss's chat whether or not the swap
            // redirects this client — a voice pad parked on the retired
            // session moves to its replacement either way.
            let rotated_session = self
                .boss_ui
                .states
                .get(&key)
                .filter(|previous| previous.identity.id == state.identity.id)
                .and_then(|previous| previous.session_id)
                .zip(state.session_id)
                .filter(|(old, new)| old != new);
            let queue_rank = boss_queue_ranks(&state);
            let rows = Arc::new(
                state
                    .outcomes
                    .iter()
                    .map(|outcome| {
                        let members: Vec<BossOutcomeMember> = state
                            .employees
                            .iter()
                            .chain(state.retired_employees.iter())
                            .filter(|employee| {
                                employee.assignment.as_ref().is_some_and(|assignment| {
                                    assignment.outcome_id == outcome.id
                                })
                            })
                            .map(|employee| BossOutcomeMember {
                                queue_rank: queue_rank.get(&employee.session_id).copied(),
                                employee: employee.clone(),
                            })
                            .collect();
                        BossOutcomeRow {
                            attention: boss_outcome_attention(outcome, &members),
                            outcome: outcome.clone(),
                            members,
                        }
                    })
                    .collect(),
            );
            self.boss_ui.outcome_rows.insert(key, rows);
            // A plan document write bumps the Boss revision — re-arm every
            // planning session's read so a waiting strip mounts its plan tab
            // once the file holds real contents.
            let planning = state.planning.clone();
            self.boss_ui.states.insert(key, state);
            if let Some((from, to)) = rotated_session {
                self.migrate_voice_scratchpad(from, to);
                self.migrate_boss_composer_draft(from, to, cx);
            }
            for plan in planning {
                self.ensure_plan_doc(key, plan.session_id, &plan.plan_file, true, cx);
            }
            changed = true;
        }
        if changed {
            self.boss_ui.hosts = self.boss_ui.states.keys().copied().collect();
            self.boss_ui.hosts.sort_by_key(|key| match key {
                DaemonKey::Local => String::new(),
                DaemonKey::Remote(id) => id.to_string(),
            });
            self.boss_ui.managed.clear();
            self.boss_ui.identities.clear();
            self.boss_ui.job_titles.clear();
            self.boss_ui.employee_icons.clear();
            self.boss_ui.working.clear();
            self.boss_ui.expired.clear();
            self.boss_ui.workspace_transitions.clear();
            self.boss_ui.queued.clear();
            self.boss_ui.dispatching.clear();
            self.boss_ui.queued_model_targets.clear();
            let session_created_at: HashMap<Uuid, u64> = self
                .state
                .sessions
                .iter()
                .map(|session| (session.id, session.created_at))
                .collect();
            for (key, state) in &self.boss_ui.states {
                if let Some(id) = state.session_id {
                    self.boss_ui.managed.insert(id);
                }
                let queue_rank = boss_queue_ranks(state);
                let mut employees = state.employees.iter().collect::<Vec<_>>();
                sort_boss_employees(&mut employees, &session_created_at);
                self.boss_ui.recent.insert(
                    *key,
                    employees.iter().map(|entry| entry.session_id).collect(),
                );
                self.boss_ui.active.insert(
                    *key,
                    employees
                        .iter()
                        .filter(|entry| !entry.expired)
                        .map(|entry| entry.session_id)
                        .collect(),
                );
                for employee in employees {
                    self.boss_ui
                        .job_titles
                        .insert(employee.session_id, employee.job_title.clone());
                    self.boss_ui
                        .employee_icons
                        .insert(employee.session_id, employee_icon(employee, state));
                    self.boss_ui.managed.insert(employee.session_id);
                    if employee.workspace_transition {
                        self.boss_ui
                            .workspace_transitions
                            .insert(employee.session_id);
                    }
                    if employee.expired {
                        self.boss_ui.expired.insert(employee.session_id);
                    } else {
                        self.boss_ui.working.insert(employee.session_id);
                    }
                    if let Some(detail) =
                        boss_queue_detail(employee, queue_rank.get(&employee.session_id).copied())
                    {
                        self.boss_ui.queued.insert(employee.session_id, detail);
                    } else if employee.lifecycle()
                        == waku_protocol::boss::EmployeeLifecycle::Dispatching
                    {
                        self.boss_ui.dispatching.insert(employee.session_id);
                    }
                    if matches!(
                        employee.lifecycle(),
                        waku_protocol::boss::EmployeeLifecycle::Queued
                            | waku_protocol::boss::EmployeeLifecycle::Dispatching
                    ) && let Some(ticket) = &employee.ticket
                    {
                        self.boss_ui.queued_model_targets.insert(
                            employee.session_id,
                            (
                                *key,
                                ticket.provider,
                                ticket.model.clone(),
                                ticket.reasoning_effort.clone(),
                            ),
                        );
                    }
                    self.boss_ui
                        .identities
                        .insert(employee.session_id, employee.identity.clone());
                }
                // Retired employees leave the visible roster but keep
                // their document record — transcript cards and mention
                // chips are historical content that still resolves them.
                // They stay boss-managed and read as finished, never
                // queued or working; the roster owns any overlap.
                for employee in &state.retired_employees {
                    self.boss_ui
                        .job_titles
                        .entry(employee.session_id)
                        .or_insert_with(|| employee.job_title.clone());
                    self.boss_ui
                        .employee_icons
                        .entry(employee.session_id)
                        .or_insert_with(|| employee_icon(employee, state));
                    self.boss_ui.managed.insert(employee.session_id);
                    self.boss_ui.expired.insert(employee.session_id);
                    self.boss_ui
                        .identities
                        .entry(employee.session_id)
                        .or_insert_with(|| employee.identity.clone());
                }
            }
            self.boss_ui.revision = self.boss_ui.revision.wrapping_add(1);
            self.sync_boss_page_rows();
            // A boss chat opening or an employee expiring can arm or
            // disarm the composer's command target.
            self.sync_composer_placeholder(cx);
            if let Some(key) = self.boss_chat_key() {
                if self
                    .boss_ui
                    .states
                    .get(&key)
                    .is_some_and(|state| !self.boss_ui.projects.contains_key(&state.identity.id))
                {
                    self.chat_with_boss(key, cx);
                }
            }
        }
        if let Some(key) = finalized_plan_host
            && let Some(session_id) = self
                .boss_ui
                .states
                .get(&key)
                .and_then(|state| state.session_id)
        {
            // Finalizing a plan returns to the boss chat at its latest
            // message, regardless of the reader's normal scroll preference.
            self.transcript_scroll_positions.remove(&session_id);
        }
        if let Some(key) = rotated_chat_host.or(finalized_plan_host) {
            self.chat_with_boss(key, cx);
        }
        self.pump_boss_avatars(cx);
        if changed {
            self.sync_boss_tasks_panel(cx);
        }
        changed
    }

    /// Re-pull every connected host's Boss document. Boss state has no
    /// broadcast — clients only load it on connect and after task-state
    /// revisions — so flipping the experiment on would otherwise leave the
    /// sidebar rows empty until one of those paths next ran.
    pub(super) fn refresh_boss_states(&mut self, cx: &mut Context<Self>) {
        for (key, _) in self.daemons.connected() {
            self.refresh_boss_state_on(key, cx);
        }
    }

    /// One `ListBuckets` pull for `key` — armed lazily when a boss
    /// surface's `@` pool draws, so a daemon without a memory engine costs
    /// one request rather than a per-keystroke miss. A failure clears the
    /// request flag so the next popup open retries.
    pub(super) fn ensure_boss_buckets(&self, key: DaemonKey, cx: &mut Context<Self>) {
        if self.boss_ui.buckets.contains_key(&key)
            || !self.boss_ui.buckets_requested.borrow_mut().insert(key)
        {
            return;
        }
        let Some(client) = self
            .daemons
            .supervisor(key)
            .map(|supervisor| supervisor.client())
        else {
            self.boss_ui.buckets_requested.borrow_mut().remove(&key);
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::Boss {
                            operation: BossOperation::Memory {
                                operation: MemoryOperation::ListBuckets,
                            },
                        },
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.boss_ui.buckets_requested.borrow_mut().remove(&key);
                if let Ok(waku_client::ResponsePayload::Boss {
                    result: BossResult::Memory { buckets: raw, .. },
                }) = result
                {
                    let buckets = raw
                        .iter()
                        .filter_map(|value| {
                            serde_json::from_value::<WireBossBucket>(value.clone()).ok()
                        })
                        .map(|bucket| BossBucketRef {
                            id: bucket.id,
                            name: bucket.name,
                            purpose: bucket.purpose,
                        })
                        .collect();
                    this.boss_ui.buckets.insert(key, Rc::new(buckets));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// One `memory/` tree pull for `key` — the mention pool's memory-file
    /// source, armed the same way [`Self::ensure_boss_buckets`] is: lazily
    /// on first pool draw, one breadth-first `ListFiles` walk per daemon,
    /// retried on the next draw after a failure.
    pub(super) fn ensure_boss_memory_files(&self, key: DaemonKey, cx: &mut Context<Self>) {
        if self.boss_ui.memory_files.contains_key(&key)
            || !self.boss_ui.memory_files_requested.borrow_mut().insert(key)
        {
            return;
        }
        let Some(client) = self
            .daemons
            .supervisor(key)
            .map(|supervisor| supervisor.client())
        else {
            self.boss_ui
                .memory_files_requested
                .borrow_mut()
                .remove(&key);
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut directories = VecDeque::from(["memory".to_owned()]);
                    let mut files = Vec::new();
                    while let Some(path) = directories.pop_front() {
                        let response = client
                            .request(
                                Uuid::nil(),
                                Uuid::nil(),
                                waku_client::Command::Boss {
                                    operation: BossOperation::ListFiles { path: path.clone() },
                                },
                            )
                            .map_err(|error| anyhow::anyhow!("{path}: {error}"))?;
                        let waku_client::ResponsePayload::Boss {
                            result: BossResult::Files { files: listing },
                        } = response
                        else {
                            anyhow::bail!("unexpected memory directory response");
                        };
                        directories.extend(
                            listing
                                .iter()
                                .filter(|file| file.directory)
                                .map(|file| file.path.clone()),
                        );
                        files.extend(listing);
                    }
                    Ok::<_, anyhow::Error>(files)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.boss_ui
                    .memory_files_requested
                    .borrow_mut()
                    .remove(&key);
                if let Ok(files) = result {
                    this.boss_ui.memory_files.insert(key, Rc::new(files));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetch one host's Boss document off the UI thread. The daemon still
    /// answers for its own copy of the experiment setting, and a returned
    /// state lands through `drain_boss_events` like any other boss reply.
    pub(super) fn refresh_boss_state_on(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
        let Some(supervisor) = self.daemons.supervisor(key) else {
            return;
        };
        let client = supervisor.client();
        let boss_updates = self.boss_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        cx.background_executor()
            .spawn(async move {
                if let Some(state) = super::runtime::load_remote_boss_state(&client) {
                    let _ = boss_updates.send((key, state));
                    signal_event_pump(&event_wake);
                }
            })
            .detach();
    }

    /// The list rows one section shows under its current filter — computed
    /// from live state so every render and revision sees the same order.
    /// Personnel rows follow `recent`'s newest-summon-first order; plans and
    /// deliverables sort newest first inside each filter.
    fn boss_page_rows(&self, key: DaemonKey, tab: BossTab) -> Vec<BossItem> {
        if tab == BossTab::Memory {
            let Some(feed) = self.boss_ui.memory_feed.get(&key) else {
                return Vec::new();
            };
            if !feed.loaded {
                return Vec::new();
            }
            let query = self.boss_ui.memory_query.trim().to_lowercase();
            let filter = self
                .boss_ui
                .memory_project
                .get(&key)
                .cloned()
                .unwrap_or_default();
            let mut shown = 0_usize;
            let mut remaining = 0_usize;
            let mut rows = Vec::new();
            for (index, record) in feed.records.iter().enumerate() {
                if !memory_record_matches(record, &query, &filter) {
                    continue;
                }
                if shown < feed.visible {
                    rows.push(BossItem::MemoryRecord(index));
                    shown += 1;
                } else {
                    remaining += 1;
                }
            }
            if remaining > 0 {
                rows.push(BossItem::MemoryFooter { remaining });
            }
            return rows;
        }
        let Some(state) = self.boss_ui.states.get(&key) else {
            return Vec::new();
        };
        match tab {
            BossTab::Memory => unreachable!(),
            BossTab::Employees => {
                let history = self
                    .boss_ui
                    .employees_view
                    .get(&key)
                    .copied()
                    .unwrap_or_default()
                    == BossEmployeesView::History;
                // An active lookup swaps the roster list for hit rows and
                // the coverage footer; an empty field with every filter
                // unset keeps the finished-roster list.
                if history && self.boss_history_search_active(key) {
                    return self.boss_history_rows(key);
                }
                let mut rows: Vec<BossItem> = self
                    .boss_ui
                    .recent
                    .get(&key)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|id| self.boss_ui.expired.contains(id) == history)
                    .map(BossItem::Employee)
                    .collect();
                if history {
                    // Retired records left the roster but their tasks stay
                    // readable — they join History below the live roster's
                    // finished rows, oldest standing last.
                    let mut retired: Vec<&waku_protocol::boss::BossEmployee> =
                        state.retired_employees.iter().collect();
                    retired.sort_by_key(|employee| {
                        std::cmp::Reverse(employee.expired_at.unwrap_or(0))
                    });
                    rows.extend(
                        retired
                            .into_iter()
                            .map(|employee| BossItem::Employee(employee.session_id)),
                    );
                }
                rows
            }
            BossTab::Personas => {
                let query = self.boss_ui.persona_query.trim().to_lowercase();
                state
                    .personas
                    .iter()
                    .filter(|persona| persona.id != state.persona_id)
                    .filter(|persona| {
                        query.is_empty() || persona.name.to_lowercase().contains(&query)
                    })
                    .map(|persona| BossItem::Persona(persona.id))
                    .collect()
            }
            BossTab::Plans => {
                let filter = self
                    .boss_ui
                    .plans_filter
                    .get(&key)
                    .copied()
                    .unwrap_or_default();
                let mut plans: Vec<&waku_protocol::boss::BossPlan> = state
                    .planning
                    .iter()
                    .filter(|plan| match filter {
                        // A draft is active while its discussion is live —
                        // the session still on record, not yet archived,
                        // and never finalized.
                        BossPlansFilter::Active => {
                            plan.finalized_at.is_none()
                                && self
                                    .state
                                    .sessions
                                    .iter()
                                    .find(|session| session.id == plan.session_id)
                                    .is_some_and(|session| session.archived_at.is_none())
                        }
                        BossPlansFilter::Approved => plan.finalized_at.is_some(),
                        BossPlansFilter::Archived => {
                            plan.finalized_at.is_none()
                                && self
                                    .state
                                    .sessions
                                    .iter()
                                    .find(|session| session.id == plan.session_id)
                                    .is_none_or(|session| session.archived_at.is_some())
                        }
                    })
                    .collect();
                plans.sort_by_key(|plan| {
                    std::cmp::Reverse(plan.finalized_at.unwrap_or_else(|| {
                        self.state
                            .sessions
                            .iter()
                            .find(|session| session.id == plan.session_id)
                            .map(|session| session.created_at)
                            .unwrap_or(0)
                    }))
                });
                plans
                    .into_iter()
                    .map(|plan| BossItem::Plan(plan.session_id))
                    .collect()
            }
            BossTab::Deliverables => {
                let filter = self
                    .boss_ui
                    .deliverables_filter
                    .get(&key)
                    .copied()
                    .unwrap_or_default();
                boss_deliverable_rows(state, filter)
            }
        }
    }

    /// Keep the virtualized list in step with the freshly computed rows —
    /// the same prefix splice the Skills list uses, so filter keystrokes
    /// and selection moves keep scroll position.
    fn sync_boss_rows(&mut self, rows: Vec<BossItem>) {
        if self.boss_ui.rows == rows {
            return;
        }
        let prefix = self
            .boss_ui
            .rows
            .iter()
            .zip(rows.iter())
            .take_while(|(cached, fresh)| cached == fresh)
            .count();
        let old_count = self.boss_ui.rows.len();
        self.boss_ui.rows = rows;
        if old_count == 0 {
            self.boss_ui.list.reset(self.boss_ui.rows.len());
        } else {
            self.boss_ui
                .list
                .splice(prefix..old_count, self.boss_ui.rows.len() - prefix);
        }
    }

    pub(super) fn sync_boss_page_rows(&mut self) {
        let Some((key, tab)) = self.boss_ui.page else {
            return;
        };
        let rows = self.boss_page_rows(key, tab);
        self.sync_boss_rows(rows);
    }

    pub(super) fn open_boss_page(
        &mut self,
        key: DaemonKey,
        tab: BossTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.boss_ui.pending {
            self.show_toast(tr!("boss.loading"));
            cx.notify();
            return;
        }
        self.session_navigation.visit(
            self.navigation_location(),
            NavigationLocation::BossPage(key, tab),
        );
        self.show_boss_page(key, tab, window, cx);
    }

    pub(super) fn show_boss_page(
        &mut self,
        key: DaemonKey,
        tab: BossTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.state.boss_experiment_enabled {
            return;
        }
        self.settings_page = None;
        self.projects_page = None;
        self.drafts_page = false;
        self.automations_page = false;
        self.notifications.open = false;
        self.selected_terminal = None;
        self.pending_session_activation = None;
        self.boss_ui.page = Some((key, tab));
        self.boss_ui.last_section.insert(key, tab);
        self.fold_terminals_group_for_navigation();
        // The composer leaves with the workspace — a hold bound to it
        // cancels rather than recording under a replaced screen.
        self.press_to_talk_navigation(cx);
        self.sync_right_panel_owner(cx);
        self.sync_boss_page_rows();
        if tab == BossTab::Memory {
            self.ensure_boss_memory_feed(key, cx);
        }
        if tab == BossTab::Employees {
            // Land on History with a live query or filters and no result —
            // arm the lookup rather than showing an empty hit list.
            self.ensure_boss_history_search(key, cx);
        }
        let focus = self
            .boss_ui
            .focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn boss_request(
        &mut self,
        key: DaemonKey,
        operation: BossOperation,
        reply: BossReply,
        cx: &mut Context<Self>,
    ) {
        if !self.state.boss_experiment_enabled {
            return;
        }
        if self.boss_ui.pending {
            // A boss-chat click while a request is in flight must not die —
            // under exactly the slowness an Open suffers, a dropped click
            // reads as a broken sidebar. A session the app already holds
            // activates straight from its copy; one it doesn't (first open,
            // or a session the catalog lost) parks the operation so the
            // completion re-issues it against the daemon. Last click wins.
            if let BossOperation::Open { .. } = operation {
                let session_id = self
                    .boss_ui
                    .states
                    .get(&key)
                    .and_then(|state| state.session_id)
                    .filter(|session_id| {
                        self.state
                            .sessions
                            .iter()
                            .any(|session| session.id == *session_id)
                    });
                if let Some(session_id) = session_id {
                    self.request_session_activation(
                        session_id,
                        SessionActivationTransition::Visit,
                        cx,
                    );
                } else {
                    self.boss_ui.queued_open = Some((key, operation));
                }
            } else if let BossOperation::HistorySearch { .. } = operation {
                // A lookup submitted mid-flight parks instead of dropping —
                // the next completion re-issues the freshest one.
                self.boss_ui.queued_history_search = Some((key, operation));
            } else if matches!(
                &operation,
                BossOperation::Memory {
                    operation: MemoryOperation::Feed
                }
            ) && reply == BossReply::MemoryFeed
            {
                // A feed refresh behind another request parks the same way —
                // the completion re-issues the freshest one.
                self.boss_ui.queued_memory_feed = Some((key, operation));
            }
            return;
        }
        let feed_request = matches!(
            &operation,
            BossOperation::Memory {
                operation: MemoryOperation::Feed
            } if reply == BossReply::MemoryFeed
        );
        let Some(client) = self
            .daemons
            .supervisor(key)
            .map(|supervisor| supervisor.client())
        else {
            let unreachable = tr!("boss.unreachable").to_string();
            match reply {
                BossReply::MemoryFeed => {
                    let feed = self.boss_ui.memory_feed.entry(key).or_default();
                    feed.loading = false;
                    feed.error = Some(unreachable);
                }
                BossReply::HistorySearch => {
                    let search = self.boss_ui.history_search.entry(key).or_default();
                    search.searching = false;
                    search.pending_offset = None;
                    search.error = Some(unreachable);
                    self.sync_boss_page_rows();
                }
                _ => self.show_toast(tr!("boss.unreachable")),
            }
            cx.notify();
            return;
        };
        let saved = matches!(reply, BossReply::Saved)
            .then(|| {
                self.boss_ui.editor.as_ref().map(|editor| {
                    (
                        [
                            &editor.name,
                            &editor.content,
                            &editor.pinned,
                            &editor.buckets,
                        ]
                        .iter()
                        .map(|input| input.read(cx).content().to_owned())
                        .collect::<Vec<_>>(),
                        editor.permissions.clone(),
                    )
                })
            })
            .flatten();
        self.boss_ui.generation = self.boss_ui.generation.wrapping_add(1);
        let generation = self.boss_ui.generation;
        self.boss_ui.pending = true;
        self.boss_ui.pending_reply = Some(reply);
        if let BossReply::Finalize(session_id) = reply {
            self.boss_ui.plan_finalizing.insert(session_id);
        }
        if feed_request {
            let feed = self
                .boss_ui
                .memory_feed
                .entry(key)
                .or_default();
            feed.loading = true;
            feed.error = None;
        }
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::Boss { operation },
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.boss_ui.generation != generation {
                    if let BossReply::Finalize(session_id) = reply {
                        this.boss_ui.plan_finalizing.remove(&session_id);
                    }
                    return;
                }
                this.boss_ui.pending = false;
                this.boss_ui.pending_reply = None;
                if reply == BossReply::MemoryFeed
                    && let Some(feed) = this.boss_ui.memory_feed.get_mut(&key)
                {
                    feed.loading = false;
                }
                // A failed or unexpected Finalize reply restores the
                // session's chip and composer so the user can retry.
                if let BossReply::Finalize(session_id) = reply
                    && !matches!(
                        &result,
                        Ok(waku_client::ResponsePayload::Boss {
                            result: BossResult::PlanFinalized { .. },
                        })
                    )
                {
                    this.boss_ui.plan_finalizing.remove(&session_id);
                }
                match result {
                    Ok(waku_client::ResponsePayload::Boss { result }) => {
                        match result {
                            BossResult::State { state } => {
                                if let (Some(editor), Some((values, _))) =
                                    (&mut this.boss_ui.editor, &saved)
                                {
                                    if editor.persona.is_nil()
                                        && let Some(persona) =
                                            state.personas.iter().rev().find(|persona| {
                                                persona.name == values[0].trim()
                                                    && persona.markdown == values[1]
                                            })
                                    {
                                        editor.persona = persona.id;
                                    }
                                }
                                let _ = this.boss_tx.send((key, state));
                                signal_event_pump(&this.event_wake_tx);
                            }
                            BossResult::Session { session, project }
                                if matches!(reply, BossReply::Open) =>
                            {
                                this.daemons.claim_project(project.id, key);
                                this.boss_ui.projects.insert(project.id, *project);
                                let rotated_from = if session.planning.is_none() {
                                    this.boss_ui
                                        .states
                                        .get_mut(&key)
                                        .and_then(|state| state.session_id.replace(session.id))
                                        .filter(|old| *old != session.id)
                                } else {
                                    None
                                };
                                let id = session.id;
                                if let Some(from) = rotated_from {
                                    // An Open answer can beat the pushed
                                    // state to the swap — carry the pad and
                                    // the draft over here too so no ordering
                                    // strands them.
                                    this.migrate_voice_scratchpad(from, id);
                                    this.migrate_boss_composer_draft(from, id, cx);
                                }
                                this.daemons.claim_session(id, key);
                                if let Some(existing) =
                                    this.state.sessions.iter_mut().find(|entry| entry.id == id)
                                {
                                    *existing = *session;
                                } else {
                                    this.state.sessions.push(*session);
                                }
                                this.request_session_activation(
                                    id,
                                    SessionActivationTransition::Visit,
                                    cx,
                                );
                            }
                            BossResult::PlanFinalized { .. }
                                if matches!(reply, BossReply::Finalize(_)) =>
                            {
                                this.chat_with_boss(key, cx);
                                this.refresh_boss_state_on(key, cx);
                            }
                            BossResult::HistorySearch { result }
                                if matches!(reply, BossReply::HistorySearch) =>
                            {
                                let search =
                                    this.boss_ui.history_search.entry(key).or_default();
                                let offset = search.pending_offset.take().unwrap_or(0);
                                if offset == 0 {
                                    search.hits = result.hits;
                                } else {
                                    merge_history_page(&mut search.hits, result.hits);
                                }
                                search.coverage = Some(result.coverage);
                                search.searching = false;
                                search.error = None;
                                this.sync_boss_page_rows();
                            }
                            BossResult::Memory { feed, .. }
                                if reply == BossReply::MemoryFeed =>
                            {
                                let records = feed
                                    .as_ref()
                                    .and_then(|feed| feed.get("records"))
                                    .and_then(|records| records.as_array())
                                    .cloned()
                                    .unwrap_or_default()
                                    .into_iter()
                                    .filter_map(|value| {
                                        serde_json::from_value::<MemoryFeedRecord>(value).ok()
                                    })
                                    .collect::<Vec<_>>();
                                this.apply_boss_memory_feed(key, records, cx);
                            }
                            _ => {}
                        }
                        if matches!(reply, BossReply::Saved) {
                            if let (Some(editor), Some((values, permissions))) =
                                (&mut this.boss_ui.editor, saved)
                            {
                                editor.original = values;
                                editor.original_permissions = permissions;
                            }
                            let saved_persona =
                                this.boss_ui.editor.as_ref().map(|editor| editor.persona);
                            if !this.boss_editor_dirty(cx) {
                                this.boss_ui.editor = None;
                            }
                            if let Some(persona) = saved_persona.filter(|id| !id.is_nil()) {
                                this.boss_ui.personas_selected.insert(key, persona);
                            }
                            this.show_toast(tr!("boss.saved"));
                            this.boss_request(key, BossOperation::View, BossReply::List, cx);
                        }
                        if matches!(reply, BossReply::PersonaDefault) {
                            this.show_toast(tr!("boss.default_saved"));
                        }
                        if matches!(reply, BossReply::BaseChoice) {
                            this.show_toast(tr!("boss.base_choice_saved"));
                        }
                        if matches!(reply, BossReply::Resume) {
                            this.show_success_toast(tr!("boss.resume_queued"));
                            // The pushed revision lands the same refresh —
                            // this pull just beats it so the control
                            // disappears with the confirmation.
                            this.refresh_boss_state_on(key, cx);
                        }
                    }
                    Ok(_) => this.show_toast(tr!("boss.unexpected_response")),
                    Err(error) => {
                        let error = error.to_string();
                        if reply == BossReply::MemoryFeed {
                            let feed = this
                                .boss_ui
                                .memory_feed
                                .entry(key)
                                .or_default();
                            feed.loading = false;
                            feed.error = Some(error);
                        } else if reply == BossReply::HistorySearch {
                            // Keep any hits already on screen — a failed
                            // lookup reports beside them, not instead of.
                            let search = this.boss_ui.history_search.entry(key).or_default();
                            search.searching = false;
                            search.pending_offset = None;
                            search.error = Some(error);
                            this.sync_boss_page_rows();
                        } else {
                            this.show_toast(tr!("boss.failed", error = error.clone()));
                        }
                    }
                }
                // A boss-chat click parked its Open while this request held
                // the pipe — re-issue it now the pipe is free. If result
                // handling already started another request, the parked op
                // stays parked and the next completion retries.
                if !this.boss_ui.pending
                    && let Some((queued_key, operation)) = this.boss_ui.queued_open.take()
                {
                    this.boss_request(queued_key, operation, BossReply::Open, cx);
                }
                if !this.boss_ui.pending
                    && let Some((queued_key, operation)) =
                        this.boss_ui.queued_history_search.take()
                {
                    this.boss_request(queued_key, operation, BossReply::HistorySearch, cx);
                }
                if !this.boss_ui.pending
                    && let Some((queued_key, operation)) =
                        this.boss_ui.queued_memory_feed.take()
                {
                    this.boss_request(queued_key, operation, BossReply::MemoryFeed, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Whether the Employees → History lookup is engaged — any field text
    /// or a set project/kind/date filter swaps the finished roster for
    /// the hit list.
    fn boss_history_search_active(&self, key: DaemonKey) -> bool {
        boss_history_search_active(
            &self.boss_ui.history_query,
            self.boss_ui.history_search.get(&key),
        )
    }

    /// The lookup's list rows — one per accumulated hit, then the
    /// coverage footer while the search has state worth reporting.
    fn boss_history_rows(&self, key: DaemonKey) -> Vec<BossItem> {
        let Some(search) = self.boss_ui.history_search.get(&key) else {
            return Vec::new();
        };
        boss_history_rows(search)
    }

    /// Enter in the History field — search the host the page shows,
    /// starting from the first page.
    pub(super) fn submit_boss_history_search(&mut self, cx: &mut Context<Self>) {
        let Some((key, BossTab::Employees)) = self.boss_ui.page else {
            return;
        };
        self.run_boss_history_search(key, 0, cx);
    }

    /// Issue the History lookup with the field's current text and the
    /// host's filter picks. `offset` continues a capped page; `0` starts
    /// fresh and replaces the accumulated hits when the reply lands. The
    /// request parks behind any in-flight boss operation rather than
    /// dropping, so a refinement submitted mid-search still runs.
    fn run_boss_history_search(&mut self, key: DaemonKey, offset: usize, cx: &mut Context<Self>) {
        if !self.state.boss_experiment_enabled {
            return;
        }
        let query = self
            .boss_history_search
            .read(cx)
            .content()
            .trim()
            .to_owned();
        let (project, kind, range) = {
            let search = self.boss_ui.history_search.entry(key).or_default();
            (search.project, search.kind, search.range)
        };
        if query.is_empty()
            && project.is_none()
            && kind.is_none()
            && range == HistoryDateRange::Any
        {
            // Nothing to ask — the view falls back to the finished roster.
            let search = self.boss_ui.history_search.entry(key).or_default();
            search.searching = false;
            search.pending_offset = None;
            return;
        }
        let (after, before) = range.bounds(unix_time());
        {
            let search = self.boss_ui.history_search.entry(key).or_default();
            search.searching = true;
            search.error = None;
            search.pending_offset = Some(offset);
        }
        self.boss_request(
            key,
            BossOperation::HistorySearch {
                query,
                project: project.map(|id| id.to_string()),
                person: None,
                after: after.map(|seconds| seconds.to_string()),
                before: before.map(|seconds| seconds.to_string()),
                kind,
                limit: Some(20),
                offset,
            },
            BossReply::HistorySearch,
            cx,
        );
        self.sync_boss_page_rows();
        cx.notify();
    }

    /// Arm the lookup when the History view opens with text or set
    /// filters but nothing resolved yet — a host the user already
    /// searched stays put.
    fn ensure_boss_history_search(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
        let history_view = self
            .boss_ui
            .employees_view
            .get(&key)
            .copied()
            .unwrap_or_default()
            == BossEmployeesView::History;
        if !history_view || !self.boss_history_search_active(key) {
            return;
        }
        let needs = self
            .boss_ui
            .history_search
            .get(&key)
            .is_none_or(|search| {
                !search.searching && search.coverage.is_none() && search.error.is_none()
            });
        if needs {
            self.run_boss_history_search(key, 0, cx);
        }
    }

    /// The projects the host's retained work spans — the filter menu's
    /// options in name order, the boss's own project included.
    fn boss_history_project_options(&self, key: DaemonKey) -> Vec<(Uuid, String)> {
        let mut options: Vec<(Uuid, String)> = self
            .state
            .projects
            .iter()
            .filter(|project| self.daemons.project_owner(project.id) == key)
            .map(|project| (project.id, project.display_name()))
            .collect();
        for project in self.boss_ui.projects.values() {
            if options.iter().all(|(id, _)| *id != project.id) {
                options.push((project.id, project.display_name()));
            }
        }
        options.sort_by_cached_key(|(_, name)| name.to_lowercase());
        options
    }

    /// Open a hit's record on its matched passage. `Inspect` keeps
    /// archived sources archived — the lookup is read-only end to end —
    /// and a record the task list no longer carries reports instead of
    /// failing silently.
    pub(super) fn open_history_hit(
        &mut self,
        task_id: Uuid,
        message_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        if !self
            .state
            .sessions
            .iter()
            .any(|session| session.id == task_id)
        {
            self.show_toast(tr!("boss.history_source_missing"));
            cx.notify();
            return;
        }
        self.pending_transcript_match = Some(PendingTranscriptMatch {
            session_id: task_id,
            message_id,
            query: String::new(),
        });
        self.request_session_activation(task_id, SessionActivationTransition::Inspect, cx);
    }

    /// The unified Memory Records feed — issued on every Memory visit so
    /// the page re-checks for newer notes. The merge decides whether the
    /// fresh page applies now or waits behind "New memories available".
    fn ensure_boss_memory_feed(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
        if !self.state.boss_experiment_enabled
            || self
                .boss_ui
                .memory_feed
                .get(&key)
                .is_some_and(|feed| feed.loading)
        {
            return;
        }
        self.boss_ui
            .memory_feed
            .entry(key)
            .or_default()
            .loading = true;
        self.boss_request(
            key,
            BossOperation::Memory {
                operation: MemoryOperation::Feed,
            },
            BossReply::MemoryFeed,
            cx,
        );
    }

    /// Merge one `Feed` reply into the host's browse state. Pure additions
    /// park behind "New memories available" so a row being read never
    /// shifts; removals and rewrites apply immediately — a record the user
    /// lost access to must not linger.
    fn apply_boss_memory_feed(
        &mut self,
        key: DaemonKey,
        fresh: Vec<MemoryFeedRecord>,
        cx: &mut Context<Self>,
    ) {
        {
            let feed = self.boss_ui.memory_feed.entry(key).or_default();
            feed.loading = false;
            feed.error = None;
            if !feed.loaded {
                feed.records = fresh;
                feed.loaded = true;
            } else {
                let fresh_keys: HashSet<String> =
                    fresh.iter().map(MemoryFeedRecord::key).collect();
                let current_keys: HashSet<String> =
                    feed.records.iter().map(MemoryFeedRecord::key).collect();
                if fresh_keys == current_keys {
                    // Identical corpus — nothing to do beyond clearing a
                    // stale "new memories" marker.
                    feed.pending_refresh.clear();
                } else if current_keys.is_subset(&fresh_keys) {
                    feed.pending_refresh = fresh;
                } else {
                    feed.records = fresh;
                    feed.pending_refresh.clear();
                    feed.expanded.retain(|key| fresh_keys.contains(key));
                }
            }
            // A project choice the fresh corpus no longer represents clears
            // to All projects — surfaced, never silent.
            let filter = self
                .boss_ui
                .memory_project
                .get(&key)
                .cloned()
                .unwrap_or_default();
            if filter != MemoryProjectFilter::All
                && !memory_project_options(&feed.records)
                    .iter()
                    .any(|(option, _)| *option == filter)
            {
                self.boss_ui.memory_project.remove(&key);
                feed.filter_notice = Some(tr!("boss.memory_filter_cleared"));
            }
        }
        self.sync_boss_page_rows();
        cx.notify();
    }

    /// Swap in the parked refresh — the "New memories available" action —
    /// and return the list to the beginning.
    fn apply_pending_memory_feed(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
        {
            let Some(feed) = self.boss_ui.memory_feed.get_mut(&key) else {
                return;
            };
            if feed.pending_refresh.is_empty() {
                return;
            }
            feed.records = std::mem::take(&mut feed.pending_refresh);
            feed.visible = MEMORY_FEED_PAGE;
            let keys: HashSet<String> = feed.records.iter().map(MemoryFeedRecord::key).collect();
            feed.expanded.retain(|key| keys.contains(key));
        }
        self.sync_boss_page_rows();
        self.boss_ui
            .list
            .scroll_to(gpui::ListOffset {
                item_ix: 0,
                offset_in_item: px(0.0),
            });
        cx.notify();
    }

    /// A search or project change restarts the browse — first page, top of
    /// the list, and any stale cleared-filter notice dismissed.
    pub(super) fn reset_boss_memory_view(&mut self, cx: &mut Context<Self>) {
        let Some((key, tab)) = self.boss_ui.page else {
            return;
        };
        if tab != BossTab::Memory {
            return;
        }
        if let Some(feed) = self.boss_ui.memory_feed.get_mut(&key) {
            feed.visible = MEMORY_FEED_PAGE;
            feed.filter_notice = None;
        }
        self.sync_boss_page_rows();
        self.boss_ui
            .list
            .scroll_to(gpui::ListOffset {
                item_ix: 0,
                offset_in_item: px(0.0),
            });
        cx.notify();
    }

    pub(super) fn boss_chat_key(&self) -> Option<DaemonKey> {
        if !self.state.boss_experiment_enabled {
            return None;
        }
        let id = self.state.selected_session?;
        self.boss_ui
            .states
            .iter()
            .find_map(|(key, state)| (state.session_id == Some(id)).then_some(*key))
    }

    /// The boss owning the selected session's surface beyond the chat
    /// itself — a planning session or deliverable page lives in the
    /// boss-minted project, so the project maps straight back to its owner
    /// the way `surface_boss_key` resolves composer pools. `None` outside
    /// boss surfaces.
    pub(super) fn selected_surface_boss_key(&self) -> Option<DaemonKey> {
        let session = self
            .state
            .sessions
            .iter()
            .find(|session| Some(session.id) == self.state.selected_session)?;
        self.boss_key_for_project(session.project_id)
    }

    /// ⌘N's recency stamps: a user send to a boss chat marks that boss the
    /// fresher destination; one that starts a New Task draft marks the page
    /// instead. Callers gate on `!submission.hidden`, so system nudges and
    /// continuations never count as either.
    pub(super) fn note_user_message_target(&mut self, session_id: Uuid) {
        let boss_key = self
            .boss_ui
            .states
            .iter()
            .find_map(|(key, state)| (state.session_id == Some(session_id)).then_some(*key));
        if let Some(key) = boss_key {
            self.boss_last_message = Some((key, unix_time()));
            return;
        }
        let draft_started = self.state.sessions.iter().any(|session| {
            session.id == session_id
                && !session.has_started()
                && !session.is_side_chat()
                && !self.session_is_boss_managed(session)
        });
        if draft_started {
            self.new_task_last_started_at = Some(unix_time());
        }
    }

    /// The daemon whose boss chat a user message last landed on, when it is
    /// fresher than the last task the user started from the New Task page —
    /// ⌘N's tie-break between the two destinations.
    pub(super) fn boss_message_outranks_new_task(&self) -> Option<DaemonKey> {
        let (key, at) = self.boss_last_message?;
        (at > self.new_task_last_started_at.unwrap_or(0)).then_some(key)
    }

    /// The boss a project-list row stands for: boss workspaces live outside
    /// `state.projects`, but the daemon mints their project id from the boss
    /// identity, so the id maps straight back to its owner — `None` unless
    /// the boss chat has actually opened its workspace.
    pub(super) fn boss_key_for_project(&self, project_id: Uuid) -> Option<DaemonKey> {
        if !self.boss_ui.projects.contains_key(&project_id) {
            return None;
        }
        self.boss_ui
            .states
            .iter()
            .find_map(|(key, state)| (state.identity.id == project_id).then_some(*key))
    }

    /// The project rows a picker adds for each live boss chat: the boss's
    /// private workspace, renamed to its identity so the row and the search
    /// read as the boss rather than a scratch directory.
    pub(super) fn boss_switcher_projects(&self) -> Vec<Project> {
        self.boss_ui
            .states
            .values()
            .filter_map(|state| {
                let mut project = self.boss_ui.projects.get(&state.identity.id)?.clone();
                project.name = state.identity.name.clone();
                Some(project)
            })
            .collect()
    }

    /// The boss chat a main-composer submission answers to while armed —
    /// a live employee's task is on screen or a sidebar deliverable was
    /// clicked — plus the context it attaches. `None` leaves the
    /// submission aimed at the selected session.
    pub(super) fn composer_boss_command(&self) -> Option<BossCommand> {
        // Big Picture retargets the composer at its own card; its
        // submission paths don't consult this either way.
        if self.big_picture.is_open() {
            return None;
        }
        if let Some((key, path, content)) = self.boss_ui.command_memory_correction.as_ref()
            && let Some(state) = self.boss_ui.states.get(key)
            && self.state.selected_session == state.session_id
            && let Some(session_id) = state.session_id
        {
            return Some(BossCommand {
                session_id,
                identity: state.identity.clone(),
                context: BossCommandContext::MemoryCorrection {
                    path: path.clone(),
                    content: content.clone(),
                },
            });
        }
        if let Some((key, deliverable_id)) = self.boss_ui.command_deliverable {
            let command = self.boss_ui.states.get(&key).and_then(|state| {
                state
                    .deliverables
                    .iter()
                    .find(|deliverable| deliverable.id == deliverable_id)
                    .and_then(|deliverable| {
                        Some(BossCommand {
                            session_id: state.session_id?,
                            identity: state.identity.clone(),
                            context: BossCommandContext::Deliverable {
                                path: PathBuf::from(&deliverable.path),
                                name: deliverable.name.clone(),
                                directory: deliverable.directory,
                            },
                        })
                    })
            });
            // A dismissed or aged-out deliverable disarms silently, falling
            // through to the viewed-employee check.
            if let Some(command) = command {
                return Some(command);
            }
        }
        let employee_id = self.state.selected_session?;
        // The user never messages an employee directly — a finished one's
        // page keeps the same composer, so expiry is not a command context
        // boundary either. Only the boss's own chat opts out: it already
        // is the destination.
        if !self.boss_ui.identities.contains_key(&employee_id) {
            return None;
        }
        let key = self.daemons.session_owner(employee_id);
        let state = self.boss_ui.states.get(&key)?;
        Some(BossCommand {
            session_id: state.session_id?,
            identity: state.identity.clone(),
            context: BossCommandContext::Employee(employee_id),
        })
    }

    /// The attachment a boss-command submission carries: a session
    /// reference for the viewed employee, the published file itself for
    /// an armed deliverable. Its mention token splices into the provider-
    /// facing prompt while the chip lands in the boss transcript's bubble.
    fn boss_command_attachment(&self, context: &BossCommandContext) -> MessageAttachment {
        match context {
            BossCommandContext::Employee(session_id) => {
                let name = self
                    .boss_ui
                    .identities
                    .get(session_id)
                    .map(|identity| identity.name.clone())
                    .unwrap_or_default();
                MessageAttachment {
                    path: PathBuf::new(),
                    mention: composer::session_token(*session_id, &name),
                    name,
                    is_dir: false,
                    is_image: false,
                    blob_reference: None,
                    pasted_text_preview: None,
                    session_id: Some(*session_id),
                }
            }
            BossCommandContext::Deliverable {
                path,
                name,
                directory,
            } => MessageAttachment {
                mention: if *directory {
                    format!("{}/", path.display())
                } else {
                    path.display().to_string()
                },
                path: path.clone(),
                name: name.clone(),
                is_dir: *directory,
                is_image: image_preview::image_format_for_name(name).is_some(),
                blob_reference: None,
                pasted_text_preview: None,
                session_id: None,
            },
            // The chip wears the entry's label; the shown text rides the
            // pasted-text preview so the user sees exactly what the Boss is
            // asked to reconcile.
            BossCommandContext::MemoryCorrection { path, content } => MessageAttachment {
                path: PathBuf::from(path),
                mention: path.clone(),
                name: path.rsplit('/').next().unwrap_or(path).to_owned(),
                is_dir: false,
                is_image: false,
                blob_reference: None,
                pasted_text_preview: Some(content.clone()),
                session_id: None,
            },
        }
    }

    /// The armed boss command's shared landing: the context attachment
    /// folds into the submission, the armed deliverable clears, and the boss
    /// chat comes on screen so the command's destination is visible.
    /// Returns the boss chat session the caller submits or steers to.
    pub(super) fn boss_command_submission_parts(
        &mut self,
        command: BossCommand,
        mut submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) -> (Uuid, ComposerSubmission) {
        let attachment = self.boss_command_attachment(&command.context);
        // The same token `merged_submission` appends: a session
        // reference's `[session ...]` form, a file's `@mention`.
        let token = composer::session_attachment_token(&attachment)
            .unwrap_or_else(|| format!("@{}", attachment.mention));
        // The bubble keeps the typed text — the context rides as its
        // attachment chip, not as mention syntax.
        if submission.display_content.is_none() {
            submission.display_content = Some(submission.prompt.trim_end().to_owned());
        }
        let prompt = submission.prompt.trim_end().to_owned();
        // A correction request reads as a request — the Boss decides how to
        // reconcile its memory; nothing claims the record changed.
        let prompt = if matches!(command.context, BossCommandContext::MemoryCorrection { .. }) {
            let instruction = "Please consider this memory correction request and update the Boss-managed memory if appropriate.";
            if prompt.is_empty() {
                instruction.to_owned()
            } else {
                format!("{instruction}\n\n{prompt}")
            }
        } else {
            prompt
        };
        submission.prompt = if prompt.is_empty() {
            token
        } else {
            format!("{prompt} {token}")
        };
        submission.attachments.push(attachment);
        self.boss_ui.command_deliverable = None;
        self.boss_ui.command_memory_correction = None;
        self.unmount_deliverable_page(cx);
        self.note_user_message_target(command.session_id);
        // Re-activating the already-viewed boss chat resets its transcript
        // before the send path can honor the reader's scroll preference.
        if self.state.selected_session != Some(command.session_id) {
            self.request_session_activation(
                command.session_id,
                SessionActivationTransition::Visit,
                cx,
            );
        }
        // Activation only syncs the hint when the session actually changed;
        // commanding the already-viewed boss chat leaves it to re-read.
        self.sync_composer_placeholder(cx);
        (command.session_id, submission)
    }

    /// Keep the composer hint honest about where Enter sends: an armed
    /// boss command goes to the boss chat — the employee on screen or the
    /// armed deliverable riding along as its attachment — everything else to
    /// the selected task.
    pub(super) fn sync_composer_placeholder(&self, cx: &mut Context<Self>) {
        // Big Picture writes its own hint while it holds the composer.
        if self.big_picture.is_open() {
            return;
        }
        let placeholder = match self.composer_boss_command().map(|command| command.context) {
            Some(BossCommandContext::Employee(..)) => tr!("boss.command_placeholder_employee"),
            Some(BossCommandContext::Deliverable { .. }) => {
                tr!("boss.command_placeholder_deliverable")
            }
            Some(BossCommandContext::MemoryCorrection { .. }) => {
                tr!("boss.command_placeholder_memory")
            }
            None => tr!("input.do_anything"),
        };
        self.composer
            .update(cx, |composer, cx| composer.set_placeholder(placeholder, cx));
    }

    /// The chip an armed main composer draws before the model picker: the
    /// boss the submission goes to, named so the destination is visible
    /// while the pickers still describe the viewed session.
    pub(super) fn render_boss_command_chip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let command = self.composer_boss_command()?;
        // The workspace footer already names the boss when the command's
        // own chat is the viewed session — one avatar and name, not two.
        if self.workspace_subject().0 == Some(command.session_id) {
            return None;
        }
        Some(
            div()
                .id("boss-command-chip")
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.0))
                .pr(px(4.0))
                .tooltip(Tooltip::text(tr!(
                    "boss.command_hint",
                    name = command.identity.name.clone()
                )))
                .child(self.boss_avatar(&command.identity, 16.0, cx))
                .child(SharedString::from(command.identity.name))
                .into_any_element(),
        )
    }

    /// The boss-side identity of a managed session: an employee's persona
    /// identity, or the boss's own for its chat and planning sessions.
    /// Plain tasks get `None`.
    pub(super) fn boss_session_identity(&self, session_id: Uuid) -> Option<BossIdentity> {
        if let Some(identity) = self.boss_ui.identities.get(&session_id) {
            return Some(identity.clone());
        }
        boss_state_owning_session(&self.boss_ui.states, session_id)
            .map(|state| state.identity.clone())
    }

    /// The employees a boss chat's transcript can name, paired with the
    /// avatar images their chips paint — fed to the markdown renderer's
    /// `with_session_mentions`.
    pub(super) fn boss_session_mentions(
        &self,
        key: DaemonKey,
    ) -> (
        Rc<Vec<md::render::SessionMention>>,
        Rc<HashMap<Uuid, Arc<gpui::RenderImage>>>,
    ) {
        let mut mentions = Vec::new();
        let mut avatars = HashMap::new();
        for id in self.boss_ui.recent.get(&key).into_iter().flatten() {
            let Some(identity) = self.boss_ui.identities.get(id) else {
                continue;
            };
            if let Some(image) = self.boss_avatar_cached(identity, MENTION_AVATAR_SIZE) {
                avatars.insert(*id, image);
            }
            mentions.push(md::render::SessionMention {
                name: SharedString::from(identity.name.clone()),
                session: *id,
            });
        }
        (Rc::new(mentions), Rc::new(avatars))
    }

    /// Every managed identity's mention avatar across bosses — the
    /// avatar half of [`Self::boss_session_mentions`] without the prose
    /// name matching. A `@`-typed session chip in any transcript paints
    /// the face the mention row and sidebar row wear; only boss surfaces
    /// additionally resolve bare names in prose.
    pub(super) fn all_mention_avatars(&self) -> Rc<HashMap<Uuid, Arc<gpui::RenderImage>>> {
        // Planning sessions stay out of the map on purpose — their chips
        // keep the compass glyph rather than the boss's face.
        let chats = self
            .boss_ui
            .states
            .values()
            .filter_map(|state| state.session_id.map(|id| (id, &state.identity)));
        let employees = self
            .boss_ui
            .identities
            .iter()
            .map(|(id, identity)| (*id, identity));
        Rc::new(
            chats
                .chain(employees)
                .filter_map(|(id, identity)| {
                    self.boss_avatar_cached(identity, MENTION_AVATAR_SIZE)
                        .map(|image| (id, image))
                })
                .collect(),
        )
    }

    /// The avatar map a composer's session atoms paint from. Unlike the
    /// mention pools this sweeps only the sessions the atoms name — a
    /// handful per field — so a miss may queue a render the way a visible
    /// surface's does.
    pub(super) fn session_atom_avatars(
        &self,
        atoms: &[composer::ComposerInlineAtom],
    ) -> Rc<HashMap<Uuid, Arc<gpui::RenderImage>>> {
        Rc::new(
            atoms
                .iter()
                .filter_map(composer::ComposerInlineAtom::session_id)
                .filter_map(|id| {
                    let identity = self.boss_session_identity(id)?;
                    self.boss_avatar_image(&identity, MENTION_AVATAR_SIZE)
                        .map(|image| (id, image))
                })
                .collect(),
        )
    }

    /// The session-chip glyph overrides every transcript shares —
    /// planning sessions paint the compass wherever a chip names them,
    /// matching the sidebar row and the `@` row that staged the mention.
    pub(super) fn mention_glyphs(&self) -> Rc<HashMap<Uuid, &'static str>> {
        Rc::new(
            self.state
                .sessions
                .iter()
                .filter(|session| session.is_planning())
                .map(|session| (session.id, crate::input::ATOM_PLANNING_ICON))
                .collect(),
        )
    }

    /// The `(daemon, identity, job title)` a managed session's top bar
    /// draws — the boss's own for its chat session, the employee's for
    /// theirs. `None` for unmanaged sessions and daemons that have not
    /// reported Boss state yet.
    pub(super) fn managed_session_meta(
        &self,
        session_id: Uuid,
    ) -> Option<(DaemonKey, BossIdentity, String)> {
        self.boss_ui.states.iter().find_map(|(key, state)| {
            if state.session_id == Some(session_id) {
                return Some((*key, state.identity.clone(), tr!("boss.group")));
            }
            state
                .employees
                .iter()
                .find(|employee| employee.session_id == session_id)
                .map(|employee| (*key, employee.identity.clone(), employee.job_title.clone()))
        })
    }

    /// Whether the session belongs to a boss — its chat or a summoned
    /// employee — including after the employee left the roster. The
    /// session's `boss_managed` stamp survives retirement, where
    /// `managed` membership does not; the set still answers for daemons
    /// that predate the stamp.
    pub(super) fn session_is_boss_managed(&self, session: &AgentSession) -> bool {
        self.state.boss_experiment_enabled
            && (session.boss_managed || self.boss_ui.managed.contains(&session.id))
    }

    /// Whether the session is a boss-owned surface rather than an
    /// employee's task — the boss's own chat or a managed session in a
    /// boss-owned kind. The boss chat never joins `managed`, so it
    /// matches on the boss state's `session_id`; managed kinds read
    /// through `session_kind_is_boss_owned`.
    pub(super) fn session_is_boss_owned(&self, session: &AgentSession) -> bool {
        if !self.state.boss_experiment_enabled {
            return false;
        }
        self.boss_ui
            .states
            .values()
            .any(|state| state.session_id == Some(session.id))
            || Self::session_kind_is_boss_owned(session)
    }

    /// Managed-session kinds that are boss surfaces rather than employee
    /// tasks. Planning sessions read the `planning` marker on the session
    /// skeleton, so `session_is_boss_owned` covers planning transcripts
    /// without a hydrate.
    fn session_kind_is_boss_owned(session: &AgentSession) -> bool {
        session.is_planning()
    }

    /// Whether the session is a summoned employee — managed by a boss but
    /// not one of the boss's own surfaces (its chat, a planning session).
    /// The `boss_managed` stamp keeps retired employees covered after they
    /// leave the daemon's roster.
    pub(super) fn session_is_employee(&self, session: &AgentSession) -> bool {
        employee_session(
            session,
            &self.boss_ui.managed,
            self.boss_ui
                .states
                .values()
                .any(|state| state.session_id == Some(session.id)),
            self.state.boss_experiment_enabled,
        )
    }

    /// Whether the session is an archived employee task — the read the
    /// activation path's restore decision and the top bar's Archived chip
    /// share.
    pub(super) fn session_is_archived_employee(&self, session: &AgentSession) -> bool {
        archived_employee_session(
            session,
            &self.boss_ui.managed,
            self.boss_ui
                .states
                .values()
                .any(|state| state.session_id == Some(session.id)),
            self.state.boss_experiment_enabled,
        )
    }

    fn managed_session_is_boss(&self, key: DaemonKey, session_id: Uuid) -> bool {
        self.boss_ui
            .states
            .get(&key)
            .is_some_and(|state| state.session_id == Some(session_id))
    }

    /// Rename the managed session's identity — the boss through `Rename`,
    /// an employee through `RenameEmployee` — so the sidebar, top bar, and
    /// session title all move together.
    pub(super) fn rename_managed_session(
        &mut self,
        session_id: Uuid,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let Some((key, identity, _)) = self.managed_session_meta(session_id) else {
            return;
        };
        if identity.name == name {
            return;
        }
        let operation = if self.managed_session_is_boss(key, session_id) {
            BossOperation::Rename { name }
        } else {
            BossOperation::RenameEmployee { session_id, name }
        };
        self.boss_request(key, operation, BossReply::List, cx);
    }

    /// Re-roll the managed session's avatar seed — the top bar's
    /// double-click on the face. The boss passes `None`; an employee its
    /// own session id.
    pub(super) fn regenerate_managed_avatar(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let Some((key, ..)) = self.managed_session_meta(session_id) else {
            return;
        };
        let employee = (!self.managed_session_is_boss(key, session_id)).then_some(session_id);
        self.boss_request(
            key,
            BossOperation::RegenerateAvatar {
                session_id: employee,
            },
            BossReply::List,
            cx,
        );
    }

    /// The generator style is a global Boss setting shared by the boss and
    /// every employee.
    pub(super) fn set_boss_avatar_style(
        &mut self,
        key: DaemonKey,
        avatar_style: AvatarStyle,
        cx: &mut Context<Self>,
    ) {
        self.boss_request(
            key,
            global_avatar_style_operation(avatar_style),
            BossReply::List,
            cx,
        );
    }

    /// The identity block a managed session's top bar shows in place of the
    /// plain title: avatar, name, and job title. Double-clicking the name
    /// swaps in the shared inline rename field; double-clicking the avatar
    /// deals a new face. Both carry focus stops so the actions are also
    /// keyboard-operable.
    pub(super) fn render_managed_session_identity(
        &self,
        session_id: Uuid,
        identity: &BossIdentity,
        job_title: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let avatar_focus = self.transcript_control_focus("managed-avatar-regenerate", cx);
        let avatar = div()
            .id(SharedString::from(format!("managed-avatar-{session_id}")))
            .track_focus(&avatar_focus)
            .tab_index(0)
            .flex_none()
            .rounded(px(8.0))
            .cursor_default()
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .tooltip(Tooltip::text(tr!("boss.new_face")))
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                if event.click_count() == 2 {
                    this.regenerate_managed_avatar(session_id, cx);
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.regenerate_managed_avatar(session_id, cx);
                    cx.stop_propagation();
                }
            }))
            .child(self.boss_avatar(identity, 24.0, cx));
        let name: AnyElement = if self.session_rename == Some(session_id) {
            div()
                .id(SharedString::from(format!(
                    "session-rename-field-{session_id}"
                )))
                .key_context(sidebar::SESSION_RENAME_PARENT_CONTEXT)
                .on_action(
                    cx.listener(|this, _: &sidebar::CancelSessionRename, window, cx| {
                        this.cancel_session_rename(window, cx);
                    }),
                )
                .h(px(22.0))
                .w(px(200.0))
                .px(px(4.0))
                .rounded(px(4.0))
                .border(hairline())
                .border_color(theme.accent)
                .bg(theme.inset)
                .flex()
                .items_center()
                .text_size(sp(13.0))
                .text_color(theme.text)
                .child(self.session_rename_input.clone())
                .into_any_element()
        } else {
            let focus = self.transcript_control_focus("managed-name-rename", cx);
            div()
                .id(SharedString::from(format!("managed-name-{session_id}")))
                .track_focus(&focus)
                .tab_index(0)
                .min_w_0()
                .truncate()
                .rounded(px(4.0))
                .text_size(sp(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .cursor_default()
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .tooltip(Tooltip::text(tr!("common.rename")))
                .on_click(
                    cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                        if event.click_count() == 2 {
                            this.begin_session_rename(session_id, window, cx);
                            cx.stop_propagation();
                        }
                    }),
                )
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter" {
                        this.begin_session_rename(session_id, window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(SharedString::from(identity.name.clone()))
                .into_any_element()
        };
        div()
            .flex()
            .items_center()
            .gap(px(7.0))
            .min_w_0()
            .child(avatar)
            .child(name)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(SharedString::from(job_title.to_owned())),
            )
            .children(self.employee_assignment_popover(session_id, &theme, cx))
            .children(
                self.boss_employee_record(session_id)
                    .and_then(|(key, employee)| {
                        self.employee_resume_button(
                            key,
                            session_id,
                            employee_resume_action(employee),
                            "header",
                            &theme,
                            cx,
                        )
                    }),
            )
            .into_any_element()
    }

    pub(super) fn render_boss_chat_empty_state(&self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let identity = self
            .boss_chat_key()
            .and_then(|key| self.boss_ui.states.get(&key))
            .map(|state| &state.identity);
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px_8()
            .pt(px(HEADER_HEIGHT))
            .pb(px(52.0))
            .children(identity.map(|identity| self.boss_avatar(identity, 54.0, cx)))
            .child(
                div()
                    .mt(px(14.0))
                    .text_size(sp(20.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .text_center()
                    .child(tr!("boss.greeting")),
            )
            .child(div().h(px(38.0)).flex_none())
    }

    pub(super) fn chat_with_boss(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
        if !self.state.boss_experiment_enabled {
            return;
        }
        // Leave the current deliverable mounted until session activation
        // records the departure. Clearing its arm here makes history see
        // the covered chat instead of the page the user is leaving.
        // Activation owns the page's unmount and composer draft handoff.
        self.boss_ui.pending_deliverable = None;
        self.boss_ui.command_memory_correction = None;
        self.sync_composer_placeholder(cx);
        self.open_boss_chat(key, cx);
    }

    fn open_boss_chat(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
        let provider = self
            .selected_session()
            .map(|session| session.provider)
            .unwrap_or(ProviderKind::Codex);
        let model = self
            .selected_session()
            .and_then(|session| session.model.clone());
        let mode = self
            .selected_session()
            .map(|session| session.runtime_mode)
            .unwrap_or_default();
        self.boss_request(
            key,
            BossOperation::Open {
                provider,
                model,
                mode,
            },
            BossReply::Open,
            cx,
        );
    }

    fn boss_editor_dirty(&self, cx: &App) -> bool {
        let Some(editor) = &self.boss_ui.editor else {
            return false;
        };
        let values = [
            &editor.name,
            &editor.content,
            &editor.pinned,
            &editor.buckets,
        ]
        .iter()
        .map(|input| input.read(cx).content().to_owned())
        .collect::<Vec<_>>();
        values != editor.original
            || editor.icon != editor.original_icon
            || serde_json::to_value(&editor.permissions).ok()
                != serde_json::to_value(&editor.original_permissions).ok()
    }

    /// The persona form the detail pane's Edit action opens. `persona` is
    /// `None` for a new role; every field the existing editor carried is
    /// editable — name, instructions, pinned documents, bucket and
    /// integration grants, delegation, computer use, and the icon.
    fn edit_boss_persona(
        &mut self,
        key: DaemonKey,
        persona: Option<BossPersona>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.boss_ui.pending {
            self.show_toast(tr!("boss.loading"));
            cx.notify();
            return;
        }
        if self.boss_editor_dirty(cx) {
            self.show_toast(tr!("boss.save_first"));
            cx.notify();
            return;
        }
        let mut input = |label: String, value: String, multiline: bool, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(window, cx)
                    .tab_index(0)
                    .accessibility_label(label);
                if multiline {
                    input = input.multi_line().auto_height().max_lines(16);
                }
                input.set_content(value, cx);
                input
            })
        };
        let pinned = persona
            .as_ref()
            .map(|entry| entry.pinned_files.join("\n"))
            .unwrap_or_default();
        let permissions = persona
            .as_ref()
            .map(|entry| entry.permissions.clone())
            .unwrap_or_default();
        let icon = persona.as_ref().and_then(|entry| entry.icon);
        let buckets = permissions.bucket_ids.join("\n");
        let name_value = persona
            .as_ref()
            .map(|entry| entry.name.clone())
            .unwrap_or_default();
        let content = persona
            .as_ref()
            .map(|entry| entry.markdown.clone())
            .unwrap_or_default();
        let original = vec![
            name_value.clone(),
            content.clone(),
            pinned.clone(),
            buckets.clone(),
        ];
        let name = input(tr!("boss.name_path"), name_value, false, cx);
        let content = input(tr!("boss.markdown"), content, true, cx);
        let pinned = input(tr!("boss.pinned_files"), pinned, true, cx);
        let buckets = input(tr!("boss.persona_buckets"), buckets, true, cx);
        let focus = name.read(cx).focus();
        self.boss_ui.editor = Some(BossEditor {
            key,
            persona: persona.as_ref().map(|entry| entry.id).unwrap_or_default(),
            name,
            content,
            pinned,
            buckets,
            original,
            original_permissions: permissions.clone(),
            icon,
            original_icon: icon,
            integrations: self
                .daemons
                .supervisor(key)
                .map(|supervisor| {
                    supervisor
                        .settings()
                        .integrations
                        .iter()
                        .map(|setting| setting.id.clone())
                        .collect()
                })
                .unwrap_or_default(),
            permissions,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    fn save_boss_document(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.boss_ui.editor else {
            return;
        };
        let key = editor.key;
        let name = editor.name.read(cx).content().trim().to_owned();
        let content = editor.content.read(cx).content().to_owned();
        let mut permissions = editor.permissions.clone();
        permissions.bucket_ids = lines(editor.buckets.read(cx).content());
        self.boss_request(
            key,
            BossOperation::UpsertPersona {
                persona: BossPersonaUpsert {
                    id: editor.persona,
                    name,
                    markdown: content,
                    pinned_files: lines(editor.pinned.read(cx).content()),
                    permissions,
                    // The editor tracks the icon itself — always
                    // replace rather than preserve.
                    icon: Some(editor.icon),
                },
            },
            BossReply::Saved,
            cx,
        );
    }

    /// The raster cached for `(seed, style, size bucket)` — a lookup with no side
    /// effects. Callers that sweep every managed identity (mention pools,
    /// transcript chips) read through here: queueing on their path would
    /// enqueue renders the cache cannot hold and evict the faces on-screen
    /// surfaces asked for.
    pub(super) fn boss_avatar_cached(
        &self,
        identity: &BossIdentity,
        size: f32,
    ) -> Option<Arc<gpui::RenderImage>> {
        self.boss_ui
            .avatars
            .get(&(identity.avatar_seed.clone(), identity.avatar_style))
            .and_then(|buckets| buckets.get(&avatar_bucket(size)))
            .map(|faces| faces.still.clone())
    }

    /// The Gaze layer rasters cached for `(seed, size bucket)` — `None` for
    /// other styles, for a missing still, and while the render is in flight.
    fn gaze_faces(&self, identity: &BossIdentity, size: f32) -> Option<Arc<GazeFaces>> {
        self.boss_ui
            .avatars
            .get(&(identity.avatar_seed.clone(), identity.avatar_style))
            .and_then(|buckets| buckets.get(&avatar_bucket(size)))
            .and_then(|faces| faces.gaze.clone())
    }

    /// The raster cached for `(seed, style, size bucket)`, queueing a render when
    /// it is missing. Returns `None` while the raster is in flight so
    /// callers can draw their placeholder. Failures retry up to twice and
    /// exhausted renders keep the placeholder; evicted rasters requeue on
    /// the next miss. Only surfaces bounded by what is on screen call this —
    /// all-identity sweeps go through [`Self::boss_avatar_cached`].
    pub(super) fn boss_avatar_image(
        &self,
        identity: &BossIdentity,
        size: f32,
    ) -> Option<Arc<gpui::RenderImage>> {
        if let Some(image) = self.boss_avatar_cached(identity, size) {
            return Some(image);
        }
        let bucket = avatar_bucket(size);
        if self.boss_ui.avatar_requested.borrow_mut().insert((
            (identity.avatar_seed.clone(), identity.avatar_style),
            bucket,
        )) {
            self.boss_ui.avatar_queue.borrow_mut().push_back((
                identity.avatar_seed.clone(),
                identity.avatar_style,
                bucket,
                1,
            ));
            signal_event_pump(&self.event_wake_tx);
        }
        None
    }

    pub(super) fn boss_avatar(&self, identity: &BossIdentity, size: f32, cx: &App) -> AnyElement {
        let circular = boss_moods::circular_frame(identity.avatar_style);
        if let Some(image) = self.boss_avatar_image(identity, size) {
            let image = gpui::img(image).size(px(size));
            return if circular {
                image.rounded_full().into_any_element()
            } else {
                image.rounded(px(6.0)).into_any_element()
            };
        }
        let placeholder = div().size(px(size));
        let placeholder = if circular {
            placeholder.rounded_full()
        } else {
            placeholder.rounded(px(6.0))
        };
        placeholder
            .bg(Theme::current(cx).overlay)
            .flex()
            .items_center()
            .justify_center()
            .child(identity.name.chars().next().unwrap_or('B').to_string())
            .into_any_element()
    }

    /// `boss_avatar` for surfaces that can afford a recurring pulse lease —
    /// sidebar rows and employee cards, whose host view already rebuilds for
    /// `spin_slow` loaders. A Gaze face keeps its body still while its eyes
    /// drift through the style's `look` track and blink on its `blink` clock;
    /// every other style, and every style under reduce-motion, returns the
    /// static element untouched.
    pub(super) fn boss_avatar_animated(
        &self,
        identity: &BossIdentity,
        size: f32,
        cx: &App,
    ) -> AnyElement {
        if !gaze_animates(identity.avatar_style, cx.reduce_motion()) {
            return self.boss_avatar(identity, size, cx);
        }
        let Some(faces) = self.gaze_faces(identity, size) else {
            // Layers render in the same job as the still, so a miss means the
            // render is in flight — the still or placeholder covers the wait.
            return self.boss_avatar(identity, size, cx);
        };
        let shift = gaze_eye_shift(&identity.avatar_seed);
        // The eye group rides the shared clock as an overlaid raster — `img`
        // takes no `Transformation`, so the drift moves an absolute wrapper
        // and the blink swaps which eyes raster is mounted.
        motion::pulse_elapsed(move |elapsed| gaze_eye_frame(&faces, size, elapsed, shift))
            .every(2)
            .into_any_element()
    }

    fn pump_boss_avatars(&mut self, cx: &mut Context<Self>) {
        while self.boss_ui.avatar_active < 4 {
            let Some((seed, style, bucket, attempt)) =
                self.boss_ui.avatar_queue.borrow_mut().pop_front()
            else {
                break;
            };
            self.boss_ui.avatar_active += 1;
            let renderer = cx.svg_renderer();
            let avatar_seed = seed.clone();
            cx.spawn(async move |this, cx| {
                let image = cx.background_executor().spawn(async move {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let scale = avatar_scale(bucket);
                        let svg = boss_moods::avatar_svg_for_style(&avatar_seed, style, bucket);
                        let still = renderer
                            .render_single_frame(&svg, scale)
                            .map_err(anyhow::Error::from)?;
                        // Gaze faces split off their eye group so the
                        // animated path can drift and blink it while the body
                        // stays put — three extra rasters on the same
                        // background render job.
                        let gaze = (style == AvatarStyle::Gaze)
                            .then(|| {
                                let layers = boss_moods::gaze_layers(&avatar_seed);
                                Ok::<_, anyhow::Error>(Arc::new(GazeFaces {
                                    body: renderer
                                        .render_single_frame(&layers.body, scale)
                                        .map_err(anyhow::Error::from)?,
                                    eyes: renderer
                                        .render_single_frame(&layers.eyes, scale)
                                        .map_err(anyhow::Error::from)?,
                                    eyes_closed: renderer
                                        .render_single_frame(&layers.eyes_closed, scale)
                                        .map_err(anyhow::Error::from)?,
                                }))
                            })
                            .transpose()?;
                        Ok::<_, anyhow::Error>(AvatarFaces { still, gaze })
                    }))
                    .unwrap_or_else(|panic| {
                        let message = panic
                            .downcast_ref::<String>()
                            .map(String::as_str)
                            .or_else(|| panic.downcast_ref::<&str>().copied())
                            .unwrap_or("non-string panic payload");
                        Err(anyhow::anyhow!("avatar renderer panicked: {message}"))
                    });
                    // Log off the UI thread, once per requested seed/size key;
                    // retries preserve the dedup mark and cannot flood the log.
                    if attempt == 1 {
                        if let Err(error) = &result {
                            eprintln!(
                                "Goddard: avatar render failed for seed {avatar_seed:?}, bucket {bucket} (up to {AVATAR_MAX_ATTEMPTS} attempts): {error:#}"
                            );
                        }
                    }
                    result
                }).await;
                let _ = this.update(cx, |this, cx| {
                    this.boss_ui.avatar_active -= 1;
                    match image {
                        Ok(faces) => {
                            if let Some(evicted) = this.boss_ui.cache_avatar((seed, style), bucket, faces) {
                                cx.drop_image(evicted.still, None);
                                if let Some(gaze) = evicted.gaze {
                                    cx.drop_image(gaze.body.clone(), None);
                                    cx.drop_image(gaze.eyes.clone(), None);
                                    cx.drop_image(gaze.eyes_closed.clone(), None);
                                }
                            }
                        }
                        Err(_) => this.boss_ui.retry_avatar((seed, style), bucket, attempt),
                    }
                    signal_event_pump(&this.event_wake_tx); cx.notify();
                });
            }).detach();
        }
    }

    pub(super) fn render_boss_sidebar_row(
        &self,
        key: DaemonKey,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.boss_ui.states.get(&key) else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let status_indicator = state
            .session_id
            .and_then(|id| self.state.sessions.iter().find(|session| session.id == id))
            .and_then(|session| self.session_status_indicator(session, &theme));
        // A live deliverable preview page owns the selection — its sidebar
        // row already wears the armed highlight — so the boss chat it covers
        // renders unselected even though its session is still the landing
        // underneath the page.
        let covered = self
            .live_deliverable_page()
            .is_some_and(|(page_key, _)| page_key == key);
        let selected = !covered
            && state.session_id.is_some_and(|id| {
                sidebar::sidebar_session_selected(
                    self.state.selected_session,
                    self.pending_session_activation
                        .map(|pending| pending.session_id),
                    id,
                )
            });
        let speech_visible = self.last_speech_key == Some(key)
            && (!self.last_speech_clips.is_empty()
                || self.speech_playback_key == Some(key)
                || self
                    .speech_clip_queue
                    .iter()
                    .any(|(queued, _)| *queued == key));
        let speech_playing = speech_visible
            && self.speech_playback_key == Some(key)
            && self
                .voice_briefing_playback
                .is_some_and(|playback| playback.playing);
        let speech_paused = speech_visible
            && self.speech_playback_key == Some(key)
            && self
                .voice_briefing_playback
                .is_some_and(|playback| !playback.playing);
        let machine_name = match key {
            DaemonKey::Local => None,
            DaemonKey::Remote(host) => self.remote_host_name(host),
        };
        let subtitle = machine_name.map_or_else(
            || tr!("boss.group"),
            |name| format!("{} · {name}", tr!("boss.group")),
        );
        // The row drags like a task row once the boss chat exists — the
        // reference names its session, so an unopened chat has nothing to
        // carry.
        let session_drag = state.session_id.map(|session_id| {
            composer::SidebarSessionDrag {
                session_id,
                title: SharedString::from(state.identity.name.clone()),
            }
        });
        div()
            .id(format!("boss-{key:?}"))
            .tab_index(0)
            .h(px(42.0))
            .w_full()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .when(selected, |row| row.bg(theme.sidebar_item_background))
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx))
            .when_some(session_drag, |row, drag| {
                row.on_drag(drag, |drag, _, _, cx| {
                    cx.new(|_| composer::SidebarDragChipView {
                        title: drag.title.clone(),
                        icon: crate::input::ATOM_SESSION_ICON,
                    })
                })
            })
            .child(self.boss_avatar_animated(&state.identity, 24.0, cx))
            .child(boss_sidebar_label(
                state.identity.name.clone(),
                subtitle,
                None,
                false,
                &theme,
                None,
            ))
            .when(speech_visible, |row| {
                let label = if speech_playing {
                    tr!("boss.pause_voice")
                } else if speech_paused {
                    tr!("boss.resume_voice")
                } else {
                    tr!("boss.replay_voice")
                };
                row.child(
                    div()
                        .id(format!("boss-voice-transport-{key:?}"))
                        .tab_index(0)
                        .w(px(20.0))
                        .h(px(20.0))
                        .flex_shrink_0()
                        .rounded(px(8.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .hover(|style| style.bg(theme.overlay))
                        .active(|style| style.bg(theme.overlay_strong))
                        .aria_label(label.clone())
                        .tooltip(Tooltip::text(label))
                        .on_activation(cx, move |this, _, cx| {
                            this.toggle_boss_speech_transport(key, cx)
                        })
                        .child(icon(
                            if speech_playing {
                                "icons/pause.svg"
                            } else if speech_paused {
                                "icons/play.svg"
                            } else {
                                "icons/rotate-cw.svg"
                            },
                            13.0,
                            theme.text_secondary,
                        )),
                )
            })
            .child(
                div()
                    .id(format!("boss-brain-{key:?}"))
                    .tab_index(0)
                    .w(px(20.0))
                    .h(px(20.0))
                    .flex_shrink_0()
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .hover(|style| style.bg(theme.overlay))
                    .active(|style| style.bg(theme.overlay_strong))
                    .aria_label(tr!("boss.brain_label"))
                    .tooltip(Tooltip::text(tr!("boss.brain_label")))
                    .on_activation(cx, move |this, window, cx| {
                        let section = this
                            .boss_ui
                            .last_section
                            .get(&key)
                            .copied()
                            .unwrap_or(BossTab::Memory);
                        this.open_boss_page(key, section, window, cx)
                    })
                    .child(icon("icons/brain.svg", 14.0, theme.text_secondary)),
            )
            .when_some(status_indicator, |row, indicator| row.child(indicator))
            .into_any_element()
    }

    /// The pending marker a queued employee's row and summon card wear
    /// instead of the shell session's status — an unstarted shell reads
    /// Idle, which is the finished look the queue exists to avoid. The
    /// tooltip carries the wait reason the scheduler recorded on the
    /// ticket. `slot` disambiguates the element id when the same employee
    /// shows on two surfaces at once. `None` for employees not queued.
    pub(super) fn boss_queued_indicator(
        &self,
        session_id: Uuid,
        slot: &str,
        theme: &Theme,
    ) -> Option<AnyElement> {
        let detail = self.boss_ui.queued.get(&session_id)?;
        Some(
            div()
                .id(SharedString::from(format!(
                    "boss-queued-{slot}-{session_id}"
                )))
                .flex_none()
                .size(px(12.0))
                .flex()
                .items_center()
                .justify_center()
                .tooltip(Tooltip::text(format!(
                    "{} · {}",
                    tr!("boss.goals_status_queued"),
                    detail
                )))
                .child(icon("icons/hourglass.svg", 12.0, theme.text_secondary))
                .into_any_element(),
        )
    }

    pub(super) fn render_boss_employee_row(&self, id: Uuid, cx: &mut Context<Self>) -> AnyElement {
        let Some(identity) = self.boss_ui.identities.get(&id) else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let session = self.state.sessions.iter().find(|session| session.id == id);
        // A queued employee wins over the session indicator — a requeued
        // employee's shell can still carry its last turn's verdict marker,
        // which would read as finished over the pending state.
        let status_indicator = self
            .boss_queued_indicator(id, "sidebar", &theme)
            .or_else(|| session.and_then(|session| self.session_status_indicator(session, &theme)))
            .or_else(|| {
                self.boss_ui.dispatching.contains(&id).then(|| {
                    motion::spin_slow(icon("icons/loader-circle.svg", 12.0, theme.text_secondary))
                })
            });
        let selected = sidebar::sidebar_session_selected(
            self.state.selected_session,
            self.pending_session_activation
                .map(|pending| pending.session_id),
            id,
        );
        // Option trades the job title for the employee's model and effort,
        // the same reveal a task row's detail line performs.
        let alt_held = self.sidebar_alt_held;
        let (detail, detail_provider) = if alt_held {
            self.boss_ui.queued_model_targets.get(&id).map_or_else(
                || {
                    (
                        session.map(|session| self.session_sidebar_model_detail(session)),
                        session.map(|session| session.provider),
                    )
                },
                |(key, provider, model, effort)| {
                    let name = self.model_display_name_on(*key, *provider, Some(model));
                    (
                        Some(match effort.as_deref() {
                            Some(effort) => format!(
                                "{name} · {}",
                                self.reasoning_effort_label_on(
                                    *key,
                                    *provider,
                                    Some(model),
                                    effort,
                                )
                            ),
                            None => name,
                        }),
                        Some(*provider),
                    )
                },
            )
        } else {
            (
                if self.boss_ui.workspace_transitions.contains(&id) {
                    Some(tr!("boss.status_switching_workspace"))
                } else {
                    self.boss_ui.job_titles.get(&id).cloned()
                },
                None,
            )
        };
        div()
            .id(format!("boss-employee-{id}"))
            .tab_index(0)
            .h(px(42.0))
            .w_full()
            .pl(px(sidebar::SIDEBAR_GROUP_CHILD_PADDING))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .when(selected, |row| row.bg(theme.sidebar_item_background))
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| {
                this.request_session_activation(id, SessionActivationTransition::Visit, cx)
            })
            .on_drag(
                composer::SidebarSessionDrag {
                    session_id: id,
                    title: SharedString::from(identity.name.clone()),
                },
                |drag, _, _, cx| {
                    cx.new(|_| composer::SidebarDragChipView {
                        title: drag.title.clone(),
                        icon: crate::input::ATOM_SESSION_ICON,
                    })
                },
            )
            .child(self.boss_avatar_animated(identity, 24.0, cx))
            .child(boss_sidebar_label(
                identity.name.clone(),
                detail.unwrap_or_default(),
                if alt_held {
                    None
                } else {
                    self.boss_ui
                        .employee_icons
                        .get(&id)
                        .copied()
                        .flatten()
                        .map(crate::custom_commands::icon_path)
                },
                true,
                &theme,
                detail_provider,
            ))
            .when_some(status_indicator, |row, indicator| row.child(indicator))
            .into_any_element()
    }

    /// A planning session's sidebar row — the boss section's session row
    /// without a persona: the plan's idea on the title line and a compass
    /// plus the localized kind label in the detail slot an employee spends
    /// on its job title.
    pub(super) fn render_boss_planning_row(
        &self,
        id: Uuid,
        shortcut_index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(session) = self.state.sessions.iter().find(|session| session.id == id) else {
            return div().into_any_element();
        };
        let Some(planning) = session.planning.as_ref() else {
            return div().into_any_element();
        };
        let status_indicator = self.session_status_indicator(session, &theme);
        let selected = sidebar::sidebar_session_selected(
            self.state.selected_session,
            self.pending_session_activation
                .map(|pending| pending.session_id),
            id,
        );
        let waku = cx.entity().downgrade();
        let menu = self.menu_handle(format!("boss-planning-{id}"), cx);
        let row_focus = menu.trigger_focus_handle().clone();
        let keyboard_menu = menu.clone();
        let row = div()
            .id(format!("boss-planning-{id}"))
            .track_focus(&row_focus)
            .tab_index(0)
            .h(px(42.0))
            .w_full()
            .pl(px(sidebar::SIDEBAR_GROUP_CHILD_PADDING))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .when(selected, |row| row.bg(theme.sidebar_item_background))
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| {
                this.request_session_activation(id, SessionActivationTransition::Visit, cx)
            })
            .on_drag(
                composer::SidebarSessionDrag {
                    session_id: id,
                    title: SharedString::from(planning.idea.clone()),
                },
                |drag, _, _, cx| {
                    cx.new(|_| composer::SidebarDragChipView {
                        title: drag.title.clone(),
                        icon: crate::input::ATOM_PLANNING_ICON,
                    })
                },
            )
            .on_key_down(cx.listener(move |_, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.as_str() == "f10" && event.keystroke.modifiers.shift {
                    keyboard_menu.open_context_menu(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(boss_sidebar_label(
                planning.idea.clone(),
                planning.label.render(),
                Some("icons/compass.svg"),
                true,
                &theme,
                None,
            ))
            .when_some(status_indicator, |row, indicator| row.child(indicator))
            .when_some(shortcut_index, |row, index| {
                row.child(
                    div()
                        .text_size(sp(11.0))
                        .text_color(theme.text_tertiary)
                        .child(sidebar::sidebar_shortcut_chip_label(index)),
                )
            });
        // The row's only menu gesture is the dismissal a task row's Archive
        // performs — an abandoned plan leaves the sidebar by the same flag
        // the post-finalization grace sweep sets.
        context_menu(
            div().w_full().child(row),
            SharedString::from(format!("boss-planning-menu-{id}")),
            &menu,
            move |_cx| {
                let archive_waku = waku.clone();
                vec![
                    MenuItem::new(tr!("session.archive"), move |window, cx| {
                        let _ = archive_waku.update(cx, |waku, cx| {
                            waku.archive_session(id, window, cx);
                        });
                    })
                    .shortcut_action(&ArchiveSession)
                    .icon("icons/archive.svg"),
                ]
            },
        )
    }

    /// A Recent deliverables row: the card chrome matches a task row, with the
    /// deliverable's display name on the title line and its file name in the
    /// detail slot a task would spend on its project. Everything the row
    /// needs was recorded at publish — no filesystem reads on the frame path.
    pub(super) fn render_sidebar_deliverable_row(
        &self,
        key: DaemonKey,
        deliverable_id: Uuid,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(deliverable) = self.boss_ui.states.get(&key).and_then(|state| {
            state
                .deliverables
                .iter()
                .find(|deliverable| deliverable.id == deliverable_id)
        }) else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let path = PathBuf::from(&deliverable.path);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&deliverable.path)
            .to_owned();
        let detail_icon = if deliverable.directory {
            "icons/folder.svg"
        } else {
            right_panel::file_icon_for_path(&deliverable.path)
        };
        // A remote daemon's path names its host's filesystem — the row keeps
        // the host in the detail line rather than pretending it is local.
        let file_name = match key {
            DaemonKey::Remote(host) => match self.remote_host_name(host) {
                Some(host) => format!("{file_name} · {host}"),
                None => file_name,
            },
            DaemonKey::Local => file_name,
        };
        let age = sidebar::format_time_ago(unix_time().saturating_sub(deliverable.updated_at));
        // Unread reads like a task's unseen completion: never opened, or
        // refreshed by a re-publish since the last open.
        let unread = deliverable
            .viewed_at
            .is_none_or(|viewed| viewed < deliverable.updated_at);
        let pinned = deliverable.pinned_at.is_some();
        let dormant = deliverable.dormant_at.is_some() && !pinned;
        let menu = self.menu_handle(format!("deliverable-{key:?}-{deliverable_id}"), cx);
        let keyboard_menu = menu.clone();
        let row_focus = menu.trigger_focus_handle().clone();
        // A clicked deliverable opens its task page — the boss chat that
        // published it — with the file armed as composer context; a
        // previewable local file also opens in the strip's file viewer
        // once the chat lands. The row keeps a selected highlight until a
        // send or session switch clears the arm.
        let armed = self.boss_ui.command_deliverable == Some((key, deliverable_id));
        let group_name = SharedString::from(format!("deliverable-row-{key:?}-{deliverable_id}"));
        // The row drags like a task row: the drop stages a deliverable
        // reference chip, so the payload carries the publishing daemon and
        // the plain file name the `@` row shows as its detail.
        let deliverable_drag = composer::SidebarDeliverableDrag {
            key,
            deliverable_id,
            name: SharedString::from(deliverable.name.clone()),
            detail: SharedString::from(
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&deliverable.path)
                    .to_owned(),
            ),
        };
        // The mini controls share the task row's chrome: zero-width until
        // the row is hovered or the button takes keyboard focus. The
        // Finder button stays on non-Markdown files and directories; Markdown
        // opens in-app and keeps Reveal in the context menu. Remote paths
        // cannot be revealed locally.
        let markdown = !deliverable.directory
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
        let finder_button = (key == DaemonKey::Local && !markdown).then(|| {
            let focus = self
                .sidebar_deliverable_finder_focuses
                .borrow_mut()
                .entry((key, deliverable_id))
                .or_insert_with(|| cx.focus_handle())
                .clone();
            let finder_path = path.clone();
            let key_path = path.clone();
            self.sidebar_deliverable_button(
                &group_name,
                format!("deliverable-finder-{key:?}-{deliverable_id}"),
                &focus,
                "icons/folder-open.svg",
                tr!("common.reveal_in_finder"),
                cx,
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                crate::platform::reveal_in_file_manager(&finder_path, cx);
            })
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    crate::platform::reveal_in_file_manager(&key_path, cx);
                }
            })
        });
        let pin_focus = self
            .sidebar_deliverable_pin_focuses
            .borrow_mut()
            .entry((key, deliverable_id))
            .or_insert_with(|| cx.focus_handle())
            .clone();
        // Holding Option retasks the pin control the way a task row's
        // does: on a live row it sweeps to the dormant fold, on a dormant
        // row it restores.
        let pin_button = self
            .sidebar_deliverable_button(
                &group_name,
                format!("deliverable-pin-{key:?}-{deliverable_id}"),
                &pin_focus,
                if self.sidebar_alt_held {
                    if dormant {
                        "icons/rotate-cw.svg"
                    } else {
                        "icons/broom.svg"
                    }
                } else if pinned {
                    "icons/pin-filled.svg"
                } else {
                    "icons/pin.svg"
                },
                if self.sidebar_alt_held {
                    if dormant {
                        tr!("session.restore")
                    } else {
                        tr!("session.sweep")
                    }
                } else if pinned {
                    tr!("session.unpin")
                } else {
                    tr!("session.pin")
                },
                cx,
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if event.modifiers().alt || this.sidebar_alt_held {
                    this.set_deliverable_dormant(key, deliverable_id, !dormant, cx);
                } else {
                    this.set_deliverable_pinned(key, deliverable_id, !pinned, cx);
                }
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.set_deliverable_pinned(key, deliverable_id, !pinned, cx);
                    cx.stop_propagation();
                }
            }));
        let archive_focus = self
            .sidebar_deliverable_archive_focuses
            .borrow_mut()
            .entry((key, deliverable_id))
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let archive_button = self
            .sidebar_deliverable_button(
                &group_name,
                format!("deliverable-archive-{key:?}-{deliverable_id}"),
                &archive_focus,
                "icons/archive.svg",
                tr!("session.archive"),
                cx,
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.set_deliverable_archived(key, deliverable_id, true, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.set_deliverable_archived(key, deliverable_id, true, cx);
                    cx.stop_propagation();
                }
            }));
        let row = div()
            .id(SharedString::from(format!(
                "deliverable-{key:?}-{deliverable_id}"
            )))
            .group(group_name.clone())
            .w_full()
            .min_w_0()
            .pl(px(8.0))
            .pr(px(8.0))
            .py(px(7.0))
            .rounded(px(9.0))
            .cursor_default()
            .track_focus(&row_focus)
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .when(armed, |element| element.bg(theme.sidebar_item_background))
            .hover(|element| element.bg(theme.sidebar_item_background))
            .active(|element| element.bg(theme.sidebar_item_background))
            .tooltip(Tooltip::text(
                deliverable
                    .source_path
                    .clone()
                    .unwrap_or_else(|| deliverable.path.clone()),
            ))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.open_deliverable_task(key, deliverable_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let key_name = event.keystroke.key.as_str();
                if matches!(key_name, "enter" | "space") {
                    this.open_deliverable_task(key, deliverable_id, cx);
                    cx.stop_propagation();
                } else if key_name == "f10" && event.keystroke.modifiers.shift {
                    keyboard_menu.open_context_menu(window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_drag(deliverable_drag, |drag, _, _, cx| {
                cx.new(|_| composer::SidebarDragChipView {
                    title: drag.name.clone(),
                    icon: crate::input::atom_ref_icon(
                        waku_protocol::model::AtomRefKind::Deliverable,
                    ),
                })
            })
            .child(
                div()
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
                            .relative()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(sp(13.5))
                                    .text_color(theme.text)
                                    .child(deliverable.name.clone()),
                            )
                            .children(finder_button)
                            .child(pin_button)
                            .child(archive_button)
                            // On hover the unread dot gives its right-edge
                            // slot to the quick actions, matching task rows.
                            .when(unread, |line| {
                                line.child(sidebar_deliverable_unread_status_slot(
                                    group_name.clone(),
                                    SharedString::from(format!(
                                        "deliverable-unread-{key:?}-{deliverable_id}"
                                    )),
                                    div().size(px(7.0)).rounded_full().bg(theme.info),
                                ))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .min_h(sp(15.0))
                            .text_size(sp(13.0))
                            .line_height(sp(15.0))
                            .child(icon(detail_icon, 12.5, theme.text_tertiary))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .text_color(theme.text_tertiary)
                                    .child(div().min_w_0().truncate().child(file_name)),
                            )
                            .child(div().flex_1())
                            // The pin marker keeps a pinned row's state
                            // visible outside hover — the group no longer
                            // explains it the way the Pinned header does
                            // for tasks.
                            .when(pinned, |line| {
                                line.child(icon("icons/pin-filled.svg", 12.0, theme.text_tertiary))
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_tertiary)
                                    .child(age),
                            ),
                    ),
            );
        let waku = cx.entity().downgrade();
        context_menu(
            div()
                .w_full()
                .pb(px(sidebar::SIDEBAR_SESSION_ROW_GAP))
                .child(row),
            SharedString::from(format!("deliverable-menu-{key:?}-{deliverable_id}")),
            &menu,
            move |_cx| {
                let pin_waku = waku.clone();
                let sweep_waku = waku.clone();
                let archive_waku = waku.clone();
                let remove_waku = waku.clone();
                let mut items = vec![
                    MenuItem::new(tr!("deliverable.open"), {
                        let path = path.clone();
                        move |_, cx| crate::platform::open_with_default_app(&path, cx)
                    })
                    .icon("icons/external-link.svg"),
                    MenuItem::new(tr!("common.reveal_in_finder"), {
                        let path = path.clone();
                        move |_, cx| crate::platform::reveal_in_file_manager(&path, cx)
                    })
                    .icon("icons/folder-open.svg"),
                    MenuItem::Separator,
                    MenuItem::new(
                        if pinned {
                            tr!("session.unpin")
                        } else {
                            tr!("session.pin")
                        },
                        move |_, cx| {
                            let _ = pin_waku.update(cx, |waku, cx| {
                                waku.set_deliverable_pinned(key, deliverable_id, !pinned, cx);
                            });
                        },
                    )
                    .icon(if pinned {
                        "icons/pin-off.svg"
                    } else {
                        "icons/pin.svg"
                    }),
                ];
                // Sweep and restore are complementary halves of the same
                // lifecycle, the same split a task row's menu makes.
                if dormant {
                    items.push(
                        MenuItem::new(tr!("session.restore"), move |_, cx| {
                            let _ = sweep_waku.update(cx, |waku, cx| {
                                waku.set_deliverable_dormant(key, deliverable_id, false, cx);
                            });
                        })
                        .icon("icons/rotate-cw.svg"),
                    );
                } else {
                    items.push(
                        MenuItem::new(tr!("session.sweep"), move |_, cx| {
                            let _ = sweep_waku.update(cx, |waku, cx| {
                                waku.set_deliverable_dormant(key, deliverable_id, true, cx);
                            });
                        })
                        .icon("icons/broom.svg"),
                    );
                }
                items.extend([
                    MenuItem::new(tr!("session.archive"), move |_, cx| {
                        let _ = archive_waku.update(cx, |waku, cx| {
                            waku.set_deliverable_archived(key, deliverable_id, true, cx);
                        });
                    })
                    .icon("icons/archive.svg"),
                    MenuItem::Separator,
                    MenuItem::new(tr!("deliverable.dismiss"), move |_, cx| {
                        let _ = remove_waku.update(cx, |waku, cx| {
                            waku.boss_request(
                                key,
                                BossOperation::DismissDeliverable { id: deliverable_id },
                                BossReply::List,
                                cx,
                            );
                        });
                    })
                    .icon("icons/trash.svg"),
                ]);
                items
            },
        )
    }

    /// A deliverable row's hover-revealed mini control — the same zero-width
    /// chrome a task row's pin and archive buttons use: it expands under
    /// row hover or its own keyboard focus.
    fn sidebar_deliverable_button(
        &self,
        group_name: &SharedString,
        id: String,
        focus: &FocusHandle,
        icon_path: &'static str,
        tooltip_text: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        sidebar_deliverable_action_slot(group_name.clone())
            .id(SharedString::from(id))
            .track_focus(focus)
            .tab_index(0)
            .focus_visible(|style| style.w(px(20.0)).opacity(1.0).bg(theme.focus_highlight()))
            .hover(|style| style.bg(theme.overlay))
            .active(|style| style.bg(theme.overlay_strong))
            .tooltip(Tooltip::text(tooltip_text))
            .child(icon(icon_path, 12.0, theme.text_secondary))
    }

    /// A deliverable row's click or Enter: the deliverable's task page is the boss
    /// chat that published it, opened with the file armed as composer
    /// context — and, when the file is previewable, its own preview page
    /// on top of it. The arm parks in `pending_deliverable` so the session
    /// switch the open triggers cannot clear it before the chat lands.
    pub(super) fn open_deliverable_task(
        &mut self,
        key: DaemonKey,
        deliverable_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        if !self.state.boss_experiment_enabled {
            return;
        }
        let from = self.navigation_location();
        self.boss_ui
            .forget_unread_deliverable_scroll(key, deliverable_id);
        self.mark_deliverable_viewed(key, deliverable_id, cx);
        // Park before requesting Open: an in-flight Boss request can
        // activate a cached chat synchronously. Keep the outgoing page's
        // arm intact until activation records its history and files its draft.
        self.boss_ui.pending_deliverable = Some((key, deliverable_id, true, from));
        self.boss_ui.command_memory_correction = None;
        self.open_boss_chat(key, cx);
        self.sync_composer_placeholder(cx);
        cx.notify();
    }

    /// The parked preview page when it is the surface on screen — its armed
    /// deliverable's boss chat still selected. The render gate prunes stale
    /// state lazily; history reads this eagerly so a dead page never claims
    /// a back slot.
    pub(super) fn live_deliverable_page(&self) -> Option<(DaemonKey, Uuid)> {
        let page = self.boss_ui.deliverable_page?;
        (self.boss_ui.command_deliverable == Some(page)
            && self
                .boss_ui
                .states
                .get(&page.0)
                .is_some_and(|state| state.session_id == self.state.selected_session))
        .then_some(page)
    }

    /// Drop the deliverable preview page's hold on the composer's draft
    /// slot: file the visible text under the deliverable's own key, then
    /// hand the lane back to whatever draft the surface underneath owns.
    /// Returns whether a page was mounted — callers that conditionally
    /// re-point the composer skip the restore when nothing moved.
    pub(super) fn unmount_deliverable_page(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((_, deliverable_id)) = self.boss_ui.deliverable_page.take() else {
            return false;
        };
        // The page's pad keeps its transcript but loses its transient
        // chrome with the surface — a bubble or clear recovery bound to
        // the closed page must not greet the next mount.
        if let Some(scratchpad) = self.voice_scratchpads.get_mut(&deliverable_id) {
            scratchpad.press_to_talk_bubble = None;
            scratchpad.clear_undo = None;
        }
        if self
            .press_to_talk_bubble_edit
            .as_ref()
            .is_some_and(|edit| edit.owner == deliverable_id)
        {
            self.press_to_talk_bubble_edit = None;
        }
        let key = crate::persistence::ComposerDraftKey::Deliverable(deliverable_id);
        if !self.draft_key_incognito(key) {
            let draft = self.current_composer_draft(Some(key), cx);
            if self.composer_drafts.set(key, draft) {
                self.schedule_composer_draft_save(cx);
            }
        }
        self.restore_selected_composer_draft(cx);
        true
    }

    /// A history hop landing on a deliverable: the same landing
    /// `open_deliverable_task` produces — boss chat underneath, file armed,
    /// preview page mounted — but the stacks already moved, so the parked
    /// arm carries no visit flag and the chat's activation is `Silent`:
    /// recording either would re-enter the hop it restores.
    pub(super) fn show_deliverable_page(
        &mut self,
        key: DaemonKey,
        deliverable_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some(session_id) = self
            .boss_ui
            .states
            .get(&key)
            .and_then(|state| state.session_id)
            .filter(|session_id| {
                self.state
                    .sessions
                    .iter()
                    .any(|session| session.id == *session_id)
            })
        else {
            return;
        };
        self.boss_ui
            .forget_unread_deliverable_scroll(key, deliverable_id);
        self.mark_deliverable_viewed(key, deliverable_id, cx);
        // Re-arming over a mounted page would orphan its draft slot — the
        // new arm stales `live_deliverable_page` while the composer still
        // holds the outgoing page's text.
        self.unmount_deliverable_page(cx);
        self.boss_ui.command_deliverable = Some((key, deliverable_id));
        self.boss_ui.pending_deliverable = Some((key, deliverable_id, false, None));
        self.sync_composer_placeholder(cx);
        self.request_session_activation(session_id, SessionActivationTransition::Silent, cx);
        cx.notify();
    }

    /// Opening a deliverable stamps it viewed on the owning daemon; the stamped
    /// state returns through the usual revision broadcast. The stamp is
    /// fire-and-forget outside `boss_request`'s pending gate — an in-flight
    /// list or save must never drop the view, and the view must never
    /// queue a click behind them.
    fn mark_deliverable_viewed(&self, key: DaemonKey, id: Uuid, cx: &mut Context<Self>) {
        let Some(client) = self
            .daemons
            .supervisor(key)
            .map(|supervisor| supervisor.client())
        else {
            return;
        };
        cx.background_executor()
            .spawn(async move {
                let _ = client.request(
                    Uuid::nil(),
                    Uuid::nil(),
                    waku_client::Command::Boss {
                        operation: BossOperation::MarkDeliverableViewed { id },
                    },
                );
            })
            .detach();
    }

    /// The deferred half of a deliverable row click, fired when the activation
    /// it triggered lands. If the chat on screen is the deliverable's own
    /// boss, the armed composer context — which a session switch clears
    /// as belonging to the previous view — comes back, and a previewable
    /// local file takes the column as its own preview page.
    pub(super) fn complete_deliverable_activation(
        &mut self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some((key, deliverable_id, record_visit, from)) =
            self.boss_ui.pending_deliverable.take()
        else {
            return;
        };
        let Some((directory, path)) = self
            .boss_ui
            .states
            .get(&key)
            .filter(|state| state.session_id == Some(session_id))
            .and_then(|state| {
                state
                    .deliverables
                    .iter()
                    .find(|deliverable| deliverable.id == deliverable_id)
                    .map(|deliverable| (deliverable.directory, PathBuf::from(&deliverable.path)))
            })
        else {
            return;
        };
        self.boss_ui.command_deliverable = Some((key, deliverable_id));
        self.sync_composer_placeholder(cx);
        if directory || key != DaemonKey::Local {
            return;
        }
        if !right_panel::transfer_file_is_previewable(&path) {
            return;
        }
        let Some(parent) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        self.sync_right_panel_files_root(cx);
        if self.right_panel_files_root.as_deref() != Some(parent.as_path()) {
            return;
        }
        // A previewable file gets its own page — the file viewer fills the
        // boss chat's column rather than its right panel. The composer
        // keeps the armed deliverable's boss chip, so the page still reads as a
        // command to the boss with the file attached.
        self.capture_and_save_current_composer_draft(cx);
        self.boss_ui.deliverable_page = Some((key, deliverable_id));
        // The column's VoicePad owner just changed: a covered chat's pad
        // pauses and this deliverable's pad — if one exists — becomes the
        // surface's. Bubbles and holds bound to the covered composer die
        // with it.
        self.sync_voice_scratchpad_capture(cx);
        self.restore_selected_composer_draft(cx);
        if record_visit {
            // Opening the covered chat is preparation for the preview,
            // not an intermediate destination. Back returns to the surface
            // the user opened the deliverable from, including a planning chat.
            self.session_navigation
                .visit(from, NavigationLocation::Deliverable(key, deliverable_id));
        }
    }

    /// Task affordances on a deliverable row are daemon mutations — the deliverable
    /// record is boss state, so the pin, sweep, and archive the row shows
    /// round-trip through the owning daemon like a publish or dismiss.
    fn set_deliverable_pinned(
        &mut self,
        key: DaemonKey,
        id: Uuid,
        pinned: bool,
        cx: &mut Context<Self>,
    ) {
        self.boss_request(
            key,
            BossOperation::PinDeliverable { id, pinned },
            BossReply::List,
            cx,
        );
    }

    fn set_deliverable_dormant(
        &mut self,
        key: DaemonKey,
        id: Uuid,
        dormant: bool,
        cx: &mut Context<Self>,
    ) {
        self.boss_request(
            key,
            BossOperation::SweepDeliverable { id, dormant },
            BossReply::List,
            cx,
        );
    }

    fn set_deliverable_archived(
        &mut self,
        key: DaemonKey,
        id: Uuid,
        archived: bool,
        cx: &mut Context<Self>,
    ) {
        self.boss_request(
            key,
            BossOperation::ArchiveDeliverable { id, archived },
            BossReply::List,
            cx,
        );
    }

    pub(super) fn render_boss_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((key, tab)) = self.boss_ui.page else {
            return div().into_any_element();
        };
        let focus = self
            .boss_ui
            .focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let theme = Theme::current(cx);
        let rows = self.boss_page_rows(key, tab);
        self.sync_boss_rows(rows);
        let content = match tab {
            BossTab::Memory => self.render_boss_memory_section(key, window, cx),
            BossTab::Personas => self.render_boss_personas_section(key, window, cx),
            BossTab::Employees => self.render_boss_employees_section(key, cx),
            BossTab::Plans => self.render_boss_plans_section(key, cx),
            BossTab::Deliverables => self.render_boss_deliverables_section(key, cx),
        };
        div()
            .track_focus(&focus)
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.render_boss_page_header(key, cx))
            .child(self.render_boss_section_strip(key, tab, &theme, cx))
            .when(
                boss_loading_label_visible(self.boss_ui.pending),
                |element| {
                    element.child(
                        div()
                            .px(px(20.0))
                            .py(px(4.0))
                            .text_size(sp(12.0))
                            .text_color(theme.text_tertiary)
                            .child(tr!("boss.loading")),
                    )
                },
            )
            .child(content)
            .into_any_element()
    }

    /// The shared Brain header: the boss's face, name, and host on the
    /// left — the name swaps to the shared rename field while a rename is
    /// open — and the labeled Identity menu plus the Chat action on the
    /// right.
    fn render_boss_page_header(&self, key: DaemonKey, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let state = self.boss_ui.states.get(&key);
        let name = state
            .map(|state| state.identity.name.clone())
            .unwrap_or_else(|| tr!("boss.group"));
        let host = match key {
            DaemonKey::Local => tr!("boss.local_host"),
            DaemonKey::Remote(host) => self
                .remote_host_name(host)
                .unwrap_or_else(|| tr!("boss.remote_host")),
        };
        let session_id = state.and_then(|state| state.session_id);
        let renaming = session_id.is_some_and(|id| self.session_rename == Some(id));
        let name_block: AnyElement = if renaming {
            div()
                .id("boss-header-rename")
                .key_context(sidebar::SESSION_RENAME_PARENT_CONTEXT)
                .on_action(
                    cx.listener(|this, _: &sidebar::CancelSessionRename, window, cx| {
                        this.cancel_session_rename(window, cx);
                    }),
                )
                .h(px(22.0))
                .w(px(220.0))
                .px(px(4.0))
                .rounded(px(4.0))
                .border(hairline())
                .border_color(theme.accent)
                .bg(theme.inset)
                .flex()
                .items_center()
                .text_size(sp(13.0))
                .text_color(theme.text)
                .child(self.session_rename_input.clone())
                .into_any_element()
        } else {
            div()
                .min_w_0()
                .truncate()
                .text_size(sp(16.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(name)
                .into_any_element()
        };
        let identity_handle = self.menu_handle("boss-identity", cx);
        let weak = cx.entity().downgrade();
        let identity_menu = dropdown_menu(
            MenuChip::new("boss-identity-trigger")
                .icon("icons/user-round.svg", theme.text_tertiary)
                .label(tr!("boss.identity"))
                .outlined()
                .background(theme.raised)
                .selected(identity_handle.is_open()),
            "boss-identity-menu",
            &identity_handle,
            MenuAlign::BelowRight,
            move |_| {
                let rename = weak.clone();
                let face = weak.clone();
                let items = vec![
                    MenuItem::new(tr!("common.rename"), move |window, cx| {
                        let _ = rename.update(cx, |this, cx| {
                            if let Some(session_id) = session_id {
                                this.begin_session_rename(session_id, window, cx);
                            }
                        });
                    })
                    .icon("icons/pencil.svg")
                    .disabled(session_id.is_none()),
                    MenuItem::new(tr!("boss.new_face"), move |_, cx| {
                        let _ = face.update(cx, |this, cx| {
                            if let Some(session_id) = session_id {
                                this.regenerate_managed_avatar(session_id, cx);
                            }
                        });
                    })
                    .icon("icons/rotate-cw.svg")
                    .disabled(session_id.is_none()),
                ];
                items
            },
        );
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(16.0))
            .py(px(12.0))
            .border_b_1()
            .border_color(theme.border)
            .when_some(
                state.map(|state| state.identity.clone()),
                |row, identity| row.child(self.boss_avatar_animated(&identity, 24.0, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(name_block)
                    .child(
                        div()
                            .text_size(sp(11.0))
                            .text_color(theme.text_tertiary)
                            .child(host),
                    ),
            )
            .child(identity_menu)
            .child(
                boss_button("boss-chat", tr!("boss.chat"), &theme)
                    .child(icon("icons/message-square.svg", 14.0, theme.text_secondary))
                    .child(tr!("boss.chat"))
                    .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx)),
            )
            .into_any_element()
    }

    /// The fixed section navigation under the header — a strip of quiet
    /// text items, not a row of bordered action buttons.
    fn render_boss_section_strip(
        &self,
        key: DaemonKey,
        tab: BossTab,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(16.0))
            .py(px(6.0))
            .border_b_1()
            .border_color(theme.separator)
            .children(
                [
                    (BossTab::Memory, "boss.memory", "icons/brain.svg"),
                    (BossTab::Personas, "boss.personas", "icons/user-round.svg"),
                    (BossTab::Employees, "boss.history", "icons/folder-clock.svg"),
                    (BossTab::Plans, "boss.plans", "icons/map.svg"),
                    (
                        BossTab::Deliverables,
                        "boss.deliverables",
                        "icons/file-text.svg",
                    ),
                ]
                .into_iter()
                .map(|(target, label, glyph)| {
                    let selected = tab == target;
                    div()
                        .id(SharedString::from(format!("boss-section-{label}")))
                        .tab_index(0)
                        .h(px(26.0))
                        .px(px(10.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .text_size(sp(12.5))
                        .cursor_pointer()
                        .when(selected, |element| {
                            element.bg(theme.overlay).text_color(theme.text)
                        })
                        .when(!selected, |element| {
                            element
                                .text_color(theme.text_secondary)
                                .hover(|style| style.bg(theme.overlay).text_color(theme.text))
                        })
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .child(icon(
                            glyph,
                            13.0,
                            if selected {
                                theme.text_secondary
                            } else {
                                theme.text_tertiary
                            },
                        ))
                        .child(tr!(label))
                        .on_activation(cx, move |this, window, cx| {
                            this.open_boss_page(key, target, window, cx)
                        })
                }),
            )
    }

    /// A section's in-list view switch — the segmented control the
    /// Projects/Git pages use: one inset track, one raised segment for the
    /// current pick.
    fn boss_segmented<T: Copy + Eq + 'static>(
        &self,
        id: &str,
        options: Vec<(T, String)>,
        current: T,
        cx: &mut Context<Self>,
        on_pick: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) -> Div {
        let theme = Theme::current(cx);
        let on_pick = Rc::new(on_pick);
        div()
            .flex_none()
            .flex()
            .items_center()
            .rounded(px(6.0))
            .p(px(2.0))
            .bg(theme.inset)
            .children(
                options
                    .into_iter()
                    .enumerate()
                    .map(move |(index, (value, label))| {
                        let selected = value == current;
                        let on_pick = on_pick.clone();
                        div()
                            .id(SharedString::from(format!("{id}-{index}")))
                            .h(px(20.0))
                            .px(px(10.0))
                            .rounded(px(5.0))
                            .flex()
                            .items_center()
                            .text_size(sp(12.5))
                            .tab_index(0)
                            .cursor_default()
                            .focus_visible(|style| style.bg(theme.focus_highlight()))
                            .when(selected, |element| {
                                element.bg(theme.surface).text_color(theme.text)
                            })
                            .when(!selected, |element| {
                                element
                                    .text_color(theme.text_secondary)
                                    .hover(|style| style.text_color(theme.text))
                            })
                            .child(SharedString::from(label))
                            .on_activation(cx, move |this, window, cx| {
                                on_pick(this, value, window, cx)
                            })
                    }),
            )
    }

    /// The section list — virtualized rows plus the shared scrollbar, sized
    /// for whichever column width the caller wraps it in.
    fn boss_item_list(&self, cx: &mut Context<Self>) -> Div {
        let visible_rows = self.boss_ui.rows.clone();
        let weak = cx.entity().downgrade();
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                list(self.boss_ui.list.clone(), move |visible_index, _, cx| {
                    let Some(item) = visible_rows.get(visible_index).cloned() else {
                        return div().into_any_element();
                    };
                    weak.upgrade()
                        .map(|entity| entity.update(cx, |this, cx| this.render_boss_item(item, cx)))
                        .unwrap_or_else(|| div().into_any_element())
                })
                .size_full(),
            )
            .child(scrollbar::vertical(
                &self.boss_ui.list,
                &self.boss_ui.scrollbar,
            ))
    }

    // ── Memory ───────────────────────────────────────────────────────────

    /// The Memory section: one unified feed of original notes across every
    /// accessible bucket — archive-style search and project filter on top,
    /// newest-first rows beneath. Buckets survive as row labels only.
    fn render_boss_memory_section(
        &mut self,
        key: DaemonKey,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let feed = self.boss_ui.memory_feed.get(&key);
        let filter = self
            .boss_ui
            .memory_project
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let options = feed
            .map(|feed| memory_project_options(&feed.records))
            .unwrap_or_default();
        let selected_label = options
            .iter()
            .find(|(option, _)| *option == filter)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| tr!("boss.memory_all_projects"));
        let weak = cx.entity().downgrade();
        let project_handle = self.menu_handle("boss-memory-project", cx);
        let project_menu = dropdown_menu(
            MenuChip::new("boss-memory-project")
                .icon("icons/folder.svg", theme.text_tertiary)
                .label(selected_label)
                .outlined()
                .height(px(24.0))
                .selected(project_handle.is_open())
                .max_w(px(200.0))
                .flex_none(),
            "boss-memory-project-menu",
            &project_handle,
            MenuAlign::BelowLeft,
            move |_| {
                let mut items = Vec::with_capacity(options.len() + 1);
                items.push(
                    MenuItem::new(tr!("boss.memory_all_projects"), {
                        let weak = weak.clone();
                        move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.boss_ui.memory_project.remove(&key);
                                this.reset_boss_memory_view(cx);
                            });
                        }
                    })
                    .selected(filter == MemoryProjectFilter::All),
                );
                items.extend(options.iter().map(|(option, name)| {
                    let option = option.clone();
                    let picked = option == filter;
                    let weak = weak.clone();
                    MenuItem::new(name.clone(), move |_, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.boss_ui
                                .memory_project
                                .insert(key, option.clone());
                            this.reset_boss_memory_view(cx);
                        });
                    })
                    .selected(picked)
                }));
                items
            },
        );
        let toolbar = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(8.0))
            .border_b_1()
            .border_color(theme.separator)
            .child(
                TextField::new("boss-memory-search", self.boss_memory_search.clone())
                    .icon("icons/search.svg", 13.0)
                    .flex_1()
                    .min_w_0(),
            )
            .child(project_menu);

        let mut notices = div().flex_none().flex().flex_col();
        if let Some(notice) = feed.and_then(|feed| feed.filter_notice.clone()) {
            notices = notices.child(
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .text_size(sp(12.0))
                    .text_color(theme.text_tertiary)
                    .child(notice),
            );
        }
        if feed.is_some_and(|feed| !feed.pending_refresh.is_empty()) {
            notices = notices.child(
                div()
                    .mx(px(16.0))
                    .my(px(4.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(6.0))
                    .bg(theme.inset)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("boss.memory_new_available")),
                    )
                    .child(
                        boss_button(
                            "boss-memory-refresh",
                            tr!("boss.memory_refresh"),
                            &theme,
                        )
                        .child(icon("icons/rotate-cw.svg", 12.0, theme.text_secondary))
                        .child(tr!("boss.memory_refresh"))
                        .on_activation(cx, move |this, _, cx| {
                            this.apply_pending_memory_feed(key, cx);
                        }),
                    ),
            );
        }
        if let Some(error) = feed.and_then(|feed| {
            (!feed.records.is_empty()).then(|| feed.error.clone()).flatten()
        }) {
            notices = notices.child(
                div()
                    .mx(px(16.0))
                    .mb(px(4.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(6.0))
                    .bg(theme.inset)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(format!(
                                "{}{}",
                                tr!("boss.memory_stale_results"),
                                error
                            )),
                    )
                    .child(
                        boss_button("boss-memory-retry-inline", tr!("boss.retry"), &theme)
                            .child(tr!("boss.retry"))
                            .on_activation(cx, move |this, _, cx| {
                                if let Some(feed) = this.boss_ui.memory_feed.get_mut(&key) {
                                    feed.loading = false;
                                }
                                this.ensure_boss_memory_feed(key, cx);
                            }),
                    ),
            );
        }

        let header = div()
            .flex_none()
            .flex()
            .items_baseline()
            .px(px(16.0))
            .pt(px(10.0))
            .pb(px(4.0))
            .child(
                div()
                    .flex_1()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(tr!("boss.memory_recent")),
            )
            .child(
                div()
                    .text_size(sp(11.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("boss.memory_newest_first")),
            );

        let query = self.boss_ui.memory_query.trim().to_lowercase();
        let body: AnyElement = match feed {
            _ if feed.is_none_or(|feed| !feed.loaded && feed.error.is_none()) => {
                let mut loading = div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .text_color(theme.text_secondary)
                            .child(tr!("boss.memory_loading")),
                    );
                // Placeholder rows keep the layout stable while the first
                // page is in flight.
                for index in 0..3 {
                    loading = loading.child(
                        div()
                            .w(px(280.0 + (index as f32) * 40.0))
                            .max_w(px(420.0))
                            .h(px(14.0))
                            .rounded(px(4.0))
                            .bg(theme.inset),
                    );
                }
                loading.into_any_element()
            }
            Some(feed) if !feed.records.is_empty() => {
                if self.boss_ui.rows.is_empty() {
                    // Loaded but filtered empty — the state table's
                    // project/search misses.
                    let mut empty = div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(10.0))
                        .px(px(40.0));
                    if feed.loading && !query.is_empty() {
                        empty = empty.child(
                            div()
                                .text_size(sp(12.5))
                                .text_color(theme.text_secondary)
                                .child(tr!("boss.memory_searching")),
                        );
                    } else if !query.is_empty() {
                        empty = empty
                            .child(
                                div()
                                    .text_size(sp(13.0))
                                    .text_color(theme.text_secondary)
                                    .text_center()
                                    .child(tr!("boss.memory_no_match")),
                            )
                            .child(
                                boss_button(
                                    "boss-memory-clear-search",
                                    tr!("boss.memory_clear_search"),
                                    &theme,
                                )
                                .child(tr!("boss.memory_clear_search"))
                                .on_activation(cx, move |this, _, cx| {
                                    this.boss_memory_search.update(cx, |input, cx| {
                                        input.set_content(String::new(), cx);
                                    });
                                }),
                            );
                    } else {
                        empty = empty
                            .child(
                                div()
                                    .text_size(sp(13.0))
                                    .text_color(theme.text_secondary)
                                    .text_center()
                                    .child(tr!("boss.memory_no_project_records")),
                            )
                            .child(
                                boss_button(
                                    "boss-memory-all-projects",
                                    tr!("boss.memory_all_projects"),
                                    &theme,
                                )
                                .child(tr!("boss.memory_all_projects"))
                                .on_activation(cx, move |this, _, cx| {
                                    this.boss_ui.memory_project.remove(&key);
                                    this.reset_boss_memory_view(cx);
                                }),
                            );
                    }
                    empty.into_any_element()
                } else {
                    self.boss_item_list(cx).into_any_element()
                }
            }
            Some(feed) if feed.error.is_some() => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .px(px(40.0))
                .child(
                    div()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .text_center()
                        .child(feed.error.clone().unwrap_or_default()),
                )
                .child(
                    boss_button("boss-memory-retry", tr!("boss.retry"), &theme)
                        .child(tr!("boss.retry"))
                        .on_activation(cx, move |this, _, cx| {
                            if let Some(feed) = this.boss_ui.memory_feed.get_mut(&key) {
                                feed.loading = false;
                            }
                            this.ensure_boss_memory_feed(key, cx);
                        }),
                )
                .into_any_element(),
            Some(_) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.0))
                .px(px(40.0))
                .child(
                    div()
                        .text_size(sp(13.0))
                        .text_color(theme.text)
                        .child(tr!("boss.memory_empty")),
                )
                .child(
                    div()
                        .max_w(px(420.0))
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .text_center()
                        .child(tr!("boss.memory_empty_hint")),
                )
                .child(
                    boss_button("boss-memory-remember", tr!("boss.ask_remember"), &theme)
                        .child(tr!("boss.ask_remember"))
                        .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx)),
                )
                .into_any_element(),
            None => div().flex_1().into_any_element(),
        };
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(header)
            .child(notices)
            .child(body)
            .into_any_element()
    }

    // ── Employees ────────────────────────────────────────────────────────

    /// Active and History over the whole roster — running and queued work
    /// together on one side, finished and retired records on the other. A
    /// Capacity menu sits beside the switch for the queue's model limits.
    fn render_boss_employees_section(
        &mut self,
        key: DaemonKey,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let view = self
            .boss_ui
            .employees_view
            .get(&key)
            .copied()
            .unwrap_or_default();
        let filter = self.boss_segmented(
            "boss-employees-view",
            vec![
                (BossEmployeesView::Active, tr!("boss.employees_active")),
                (BossEmployeesView::History, tr!("boss.employees_history")),
            ],
            view,
            cx,
            move |this, picked, _, cx| {
                this.boss_ui.employees_view.insert(key, picked);
                this.sync_boss_page_rows();
                if picked == BossEmployeesView::History {
                    this.ensure_boss_history_search(key, cx);
                }
                cx.notify();
            },
        );
        let searching =
            view == BossEmployeesView::History && self.boss_history_search_active(key);
        let body: AnyElement = if searching {
            self.render_boss_history_body(key, &theme, cx)
        } else if self.boss_ui.rows.is_empty() {
            let (title, hint) = match view {
                BossEmployeesView::Active => (
                    tr!("boss.employees_empty_active"),
                    tr!("boss.employees_empty_active_hint"),
                ),
                BossEmployeesView::History => (
                    tr!("boss.employees_empty_history"),
                    tr!("boss.employees_empty_history_hint"),
                ),
            };
            boss_empty_state(&theme, "icons/folder-clock.svg", title, hint).into_any_element()
        } else {
            self.boss_item_list(cx).into_any_element()
        };
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(theme.separator)
                    .child(filter)
                    .child(div().flex_1())
                    .child(self.render_boss_capacity_menu(key, cx)),
            )
            .when(view == BossEmployeesView::History, |page| {
                page.child(self.render_boss_history_toolbar(key, cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH + 48.0))
                    .mx_auto()
                    .px(px(16.0))
                    .py(px(6.0))
                    .flex()
                    .flex_col()
                    .child(body),
            )
            .into_any_element()
    }

    /// The History lookup's controls — a topic-or-name field plus the
    /// project, source-kind, and date filters — beside the plain scope
    /// statement the plan asks the view to carry.
    fn render_boss_history_toolbar(&mut self, key: DaemonKey, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let search = self.boss_ui.history_search.get(&key);
        let project_filter = search.and_then(|search| search.project);
        let kind_filter = search.and_then(|search| search.kind);
        let range = search.map(|search| search.range).unwrap_or_default();

        let project_options = self.boss_history_project_options(key);
        let weak = cx.entity().downgrade();
        let project_handle = self.menu_handle("boss-history-project", cx);
        let project_label = project_filter
            .and_then(|id| {
                project_options
                    .iter()
                    .find(|(option, _)| *option == id)
                    .map(|(_, name)| name.clone())
                    .or_else(|| {
                        self.state
                            .projects
                            .iter()
                            .find(|project| project.id == id)
                            .map(|project| project.display_name())
                    })
            })
            .unwrap_or_else(|| tr!("boss.history_all_projects"));
        let project_menu = dropdown_menu(
            MenuChip::new("boss-history-project")
                .icon("icons/folder.svg", theme.text_tertiary)
                .label(project_label)
                .outlined()
                .height(px(24.0))
                .selected(project_handle.is_open())
                .max_w(px(180.0))
                .flex_none(),
            "boss-history-project-menu",
            &project_handle,
            MenuAlign::BelowLeft,
            move |_| {
                let mut items = Vec::with_capacity(project_options.len() + 1);
                items.push(
                    MenuItem::new(tr!("boss.history_all_projects"), {
                        let weak = weak.clone();
                        move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.boss_ui
                                    .history_search
                                    .entry(key)
                                    .or_default()
                                    .project = None;
                                this.run_boss_history_search(key, 0, cx);
                            });
                        }
                    })
                    .selected(project_filter.is_none()),
                );
                items.extend(project_options.iter().map(|(id, name)| {
                    let id = *id;
                    let weak = weak.clone();
                    MenuItem::new(name.clone(), move |_, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.boss_ui
                                .history_search
                                .entry(key)
                                .or_default()
                                .project = Some(id);
                            this.run_boss_history_search(key, 0, cx);
                        });
                    })
                    .selected(project_filter == Some(id))
                }));
                items
            },
        );

        let weak = cx.entity().downgrade();
        let kind_handle = self.menu_handle("boss-history-kind", cx);
        let kind_options: Vec<(Option<waku_protocol::model::HistorySourceKind>, String)> = vec![
            (None, tr!("boss.history_all_sources")),
            (
                Some(waku_protocol::model::HistorySourceKind::Task),
                tr!("boss.history_kind_task"),
            ),
            (
                Some(waku_protocol::model::HistorySourceKind::Employee),
                tr!("boss.history_kind_employee"),
            ),
            (
                Some(waku_protocol::model::HistorySourceKind::Boss),
                tr!("boss.history_kind_boss"),
            ),
            (
                Some(waku_protocol::model::HistorySourceKind::Plan),
                tr!("boss.history_kind_plan"),
            ),
        ];
        let kind_label = kind_options
            .iter()
            .find(|(kind, _)| *kind == kind_filter)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| tr!("boss.history_all_sources"));
        let kind_menu = dropdown_menu(
            MenuChip::new("boss-history-kind")
                .icon("icons/list-filter.svg", theme.text_tertiary)
                .label(kind_label)
                .outlined()
                .height(px(24.0))
                .selected(kind_handle.is_open())
                .max_w(px(160.0))
                .flex_none(),
            "boss-history-kind-menu",
            &kind_handle,
            MenuAlign::BelowLeft,
            move |_| {
                kind_options
                    .iter()
                    .map(|(kind, label)| {
                        let kind = *kind;
                        let weak = weak.clone();
                        MenuItem::new(label.clone(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.boss_ui
                                    .history_search
                                    .entry(key)
                                    .or_default()
                                    .kind = kind;
                                this.run_boss_history_search(key, 0, cx);
                            });
                        })
                        .selected(kind == kind_filter)
                    })
                    .collect()
            },
        );

        let weak = cx.entity().downgrade();
        let date_handle = self.menu_handle("boss-history-date", cx);
        let date_options: Vec<(HistoryDateRange, String)> = vec![
            (HistoryDateRange::Any, tr!("boss.history_date_any")),
            (HistoryDateRange::Last7, tr!("boss.history_date_7")),
            (HistoryDateRange::Last30, tr!("boss.history_date_30")),
            (HistoryDateRange::Last90, tr!("boss.history_date_90")),
            (HistoryDateRange::Before90, tr!("boss.history_date_older")),
        ];
        let date_label = date_options
            .iter()
            .find(|(option, _)| *option == range)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| tr!("boss.history_date_any"));
        let date_menu = dropdown_menu(
            MenuChip::new("boss-history-date")
                .icon("icons/hourglass.svg", theme.text_tertiary)
                .label(date_label)
                .outlined()
                .height(px(24.0))
                .selected(date_handle.is_open())
                .max_w(px(160.0))
                .flex_none(),
            "boss-history-date-menu",
            &date_handle,
            MenuAlign::BelowLeft,
            move |_| {
                date_options
                    .iter()
                    .map(|(option, label)| {
                        let option = *option;
                        let weak = weak.clone();
                        MenuItem::new(label.clone(), move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.boss_ui
                                    .history_search
                                    .entry(key)
                                    .or_default()
                                    .range = option;
                                this.run_boss_history_search(key, 0, cx);
                            });
                        })
                        .selected(option == range)
                    })
                    .collect()
            },
        );

        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(6.0))
            .border_b_1()
            .border_color(theme.separator)
            .child(
                TextField::new("boss-history-search", self.boss_history_search.clone())
                    .icon("icons/search.svg", 13.0)
                    .w(px(230.0))
                    .flex_none(),
            )
            .child(project_menu)
            .child(kind_menu)
            .child(date_menu)
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .text_size(sp(11.5))
                    .text_color(theme.text_ghost)
                    .child(tr!("boss.history_scope_hint")),
            )
    }

    /// The lookup's body states — searching, failure with retry, an honest
    /// no-match inside the searched scope, and the unsubmitted hint — or
    /// the hit list itself.
    fn render_boss_history_body(
        &self,
        key: DaemonKey,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let search = self.boss_ui.history_search.get(&key);
        if !self.boss_ui.rows.is_empty() {
            return self.boss_item_list(cx).into_any_element();
        }
        if search.is_some_and(|search| search.searching) {
            return boss_empty_state(
                theme,
                "icons/search.svg",
                tr!("boss.history_searching"),
                tr!("boss.history_searching_hint"),
            )
            .into_any_element();
        }
        if let Some(error) = search.and_then(|search| search.error.clone()) {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.0))
                .px(px(40.0))
                .child(
                    div()
                        .text_size(sp(13.0))
                        .text_color(theme.text_secondary)
                        .text_center()
                        .child(error),
                )
                .child(
                    boss_button("boss-history-retry", tr!("boss.retry"), theme)
                        .child(tr!("boss.retry"))
                        .on_activation(cx, move |this, _, cx| {
                            this.run_boss_history_search(key, 0, cx);
                        }),
                )
                .into_any_element();
        }
        if let Some(coverage) = search.and_then(|search| search.coverage.as_ref()) {
            // A resolved search with zero hits says what it covered rather
            // than implying the work never happened.
            return boss_empty_state(
                theme,
                "icons/search.svg",
                tr!("boss.history_no_match"),
                tr!("boss.history_no_match_hint", scope = coverage.scope.clone()),
            )
            .into_any_element();
        }
        boss_empty_state(
            theme,
            "icons/search.svg",
            tr!("boss.history_submit_hint"),
            tr!("boss.history_scope_hint"),
        )
        .into_any_element()
    }

    /// A History hit: source kind, subject, person, project and date up
    /// top, archive state labelled, and the excerpt that earned the row —
    /// flagged as orientation when the title matched instead of a passage.
    fn render_boss_history_hit(&self, key: DaemonKey, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(hit) = self
            .boss_ui
            .history_search
            .get(&key)
            .and_then(|search| search.hits.get(index))
            .cloned()
        else {
            return div().into_any_element();
        };
        use waku_protocol::model::HistorySourceKind;
        let (kind_label, kind_icon) = match hit.kind {
            HistorySourceKind::Task => (tr!("boss.history_kind_task"), "icons/message-square.svg"),
            HistorySourceKind::Employee => {
                (tr!("boss.history_kind_employee"), "icons/bot.svg")
            }
            HistorySourceKind::Boss => (tr!("boss.history_kind_boss"), "icons/goddard-logo.svg"),
            HistorySourceKind::Plan => (tr!("boss.history_kind_plan"), "icons/file-text.svg"),
        };
        let mut meta = vec![kind_label];
        if let Some(person) = hit
            .person
            .as_ref()
            .filter(|person| person.as_str() != hit.title.as_str())
        {
            meta.push(person.clone());
        }
        if let Some(job) = hit.job_title.as_ref().filter(|job| !job.is_empty()) {
            meta.push(job.clone());
        }
        if !hit.project.is_empty() {
            meta.push(hit.project.clone());
        }
        if let Some(recorded_by) = hit.recorded_by.as_ref() {
            meta.push(tr!("boss.history_recorded_by", name = recorded_by.clone()));
        }
        if hit.archived {
            meta.push(tr!("session.archived"));
        }
        if hit.employee_expired == Some(true) {
            meta.push(tr!("boss.status_expired"));
        }
        if hit.matched_messages > 1 {
            meta.push(tr!(
                "boss.history_passages",
                count = hit.matched_messages as usize
            ));
        }
        let task_id = hit.task_id;
        let message_id = hit.message_id;
        div()
            .id(boss_history_hit_id(task_id, message_id))
            .tab_index(0)
            .w_full()
            .px(px(8.0))
            .py(px(6.0))
            .flex()
            .flex_col()
            .gap(px(3.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| {
                this.open_history_hit(task_id, message_id, cx)
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .min_w_0()
                    .child(icon(kind_icon, 13.0, theme.text_tertiary).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(14.0))
                            .line_height(sp(17.0))
                            .text_color(theme.text)
                            .child(hit.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(12.0))
                            .text_color(theme.text_ghost)
                            .child(components::format_message_time(hit.updated_at)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .min_w_0()
                    .pl(px(19.0))
                    .text_size(sp(12.0))
                    .line_height(sp(15.0))
                    .text_color(theme.text_tertiary)
                    .truncate()
                    .child(meta.join(" · ")),
            )
            .child(
                div()
                    .pl(px(19.0))
                    .min_w_0()
                    .text_size(sp(12.5))
                    .line_height(sp(16.0))
                    .text_color(theme.text_secondary)
                    .line_clamp(2)
                    .text_ellipsis()
                    .child(if hit.excerpt_matched {
                        format!("“{}”", hit.excerpt.trim())
                    } else {
                        tr!(
                            "boss.history_excerpt_unmatched",
                            excerpt = hit.excerpt.trim().to_owned()
                        )
                    }),
            )
            .into_any_element()
    }

    /// The lookup's coverage row — what the reply says it searched, what
    /// the cap cut, and the caveats worth repeating — plus the Show more
    /// continuation while pages remain.
    fn render_boss_history_footer(&self, key: DaemonKey, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(search) = self.boss_ui.history_search.get(&key) else {
            return div().into_any_element();
        };
        let mut lines: Vec<AnyElement> = Vec::new();
        if let Some(coverage) = search.coverage.as_ref() {
            lines.push(
                div()
                    .truncate()
                    .child(
                        tr!(
                            "boss.history_coverage",
                            shown = search.hits.len(),
                            matched = coverage.sources_matched as usize,
                            scanned = coverage.sources_scanned as usize,
                            scope = coverage.scope.clone()
                        )
                        .to_string(),
                    )
                    .into_any_element(),
            );
            for note in &coverage.notes {
                lines.push(div().truncate().child(note.clone()).into_any_element());
            }
        }
        if let Some(error) = search.error.as_ref() {
            lines.push(
                div()
                    .text_color(theme.warning)
                    .truncate()
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        if search.searching {
            lines.push(
                div()
                    .child(tr!("boss.history_searching").to_string())
                    .into_any_element(),
            );
        }
        let next_offset = search
            .coverage
            .as_ref()
            .and_then(|coverage| coverage.next_offset);
        let show_more = next_offset.map(|offset| {
            boss_button(
                format!("boss-history-more-{key:?}"),
                tr!("boss.history_show_more"),
                &theme,
            )
            .child(tr!("boss.history_show_more"))
            .on_activation(cx, move |this, _, cx| {
                this.run_boss_history_search(key, offset as usize, cx);
            })
            .into_any_element()
        });
        let retry = (search.error.is_some() && !search.searching).then(|| {
            boss_button("boss-history-footer-retry", tr!("boss.retry"), &theme)
                .child(tr!("boss.retry"))
                .on_activation(cx, move |this, _, cx| {
                    this.run_boss_history_search(key, 0, cx);
                })
                .into_any_element()
        });
        div()
            .w_full()
            .px(px(8.0))
            .py(px(8.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .text_size(sp(11.5))
            .line_height(sp(14.0))
            .text_color(theme.text_ghost)
            .children(lines)
            .when(show_more.is_some() || retry.is_some(), |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .children(show_more)
                        .children(retry),
                )
            })
            .into_any_element()
    }

    /// The read-only Capacity menu — model limits and host resources with
    /// live usage and queued waiters, no editing.
    fn render_boss_capacity_menu(&self, key: DaemonKey, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let handle = self.menu_handle("boss-capacity", cx);
        let mut lines = Vec::new();
        let mut host_line = None;
        if let Some(state) = self.boss_ui.states.get(&key) {
            use waku_protocol::boss::EmployeeLifecycle;
            for limit in &state.resource_policy.model_limits {
                let name =
                    self.model_display_name_on(key, limit.provider, Some(limit.model.as_str()));
                let mut live = 0usize;
                let mut queued = 0usize;
                for employee in &state.employees {
                    let Some(ticket) = &employee.ticket else {
                        continue;
                    };
                    if ticket.provider != limit.provider || ticket.model != limit.model {
                        continue;
                    }
                    match employee.lifecycle() {
                        EmployeeLifecycle::Queued => queued += 1,
                        EmployeeLifecycle::Dispatching
                        | EmployeeLifecycle::Working
                        | EmployeeLifecycle::Finishing => live += 1,
                        EmployeeLifecycle::Expired => {}
                    }
                }
                lines.push(if limit.live_limit == 0 && limit.hard_cap == 0 {
                    tr!("boss.capacity_paused", model = name)
                } else if queued > 0 {
                    tr!(
                        "boss.capacity_line_queued",
                        model = name,
                        used = live,
                        limit = limit.live_limit,
                        queued = queued
                    )
                } else {
                    tr!(
                        "boss.capacity_line",
                        model = name,
                        used = live,
                        limit = limit.live_limit
                    )
                });
            }
            if let Some(host) = &state.resource_policy.host {
                host_line = Some(tr!(
                    "boss.capacity_host_line",
                    devices = host.resident_devices,
                    builds = host.native_builds,
                    desktop = host.desktop_input
                ));
            }
        }
        dropdown_menu(
            MenuChip::new("boss-capacity-trigger")
                .icon("icons/gauge.svg", theme.text_tertiary)
                .label(tr!("boss.capacity"))
                .outlined()
                .background(theme.raised)
                .height(px(24.0))
                .selected(handle.is_open()),
            "boss-capacity-menu",
            &handle,
            MenuAlign::BelowRight,
            move |_| {
                let mut items = vec![MenuItem::Header(tr!("boss.capacity_models").into())];
                if lines.is_empty() {
                    items.push(MenuItem::new(tr!("boss.capacity_none"), |_, _| {}).disabled(true));
                }
                for line in &lines {
                    items.push(MenuItem::new(line.clone(), |_, _| {}).disabled(true));
                }
                if let Some(host_line) = host_line.as_ref() {
                    items.push(MenuItem::Separator);
                    items.push(MenuItem::Header(tr!("boss.capacity_host").into()));
                    items.push(MenuItem::new(host_line.clone(), |_, _| {}).disabled(true));
                }
                items
            },
        )
    }

    /// A full-width Employees row: avatar and name over job · project, with
    /// model · effort, a readable status, and the summon or finish age on
    /// the right — no Option reveal needed for the model. Queued rows
    /// carry their wait reason inline after the status word.
    fn render_boss_employee_page_row(
        &self,
        id: Uuid,
        key: DaemonKey,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(identity) = self.boss_ui.identities.get(&id).cloned() else {
            return div().into_any_element();
        };
        let state = self.boss_ui.states.get(&key);
        let employee = state.and_then(|state| {
            state
                .employees
                .iter()
                .chain(state.retired_employees.iter())
                .find(|employee| employee.session_id == id)
        });
        let session = self.state.sessions.iter().find(|session| session.id == id);
        let job = self
            .boss_ui
            .job_titles
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let project = session
            .and_then(|session| {
                self.state
                    .projects
                    .iter()
                    .find(|project| project.id == session.project_id)
            })
            .map(|project| project.display_name())
            .or_else(|| {
                employee
                    .and_then(|employee| employee.ticket.as_ref())
                    .map(|ticket| ticket.project.trim().to_owned())
                    .filter(|project| !project.is_empty())
            });
        let mut detail = job.clone();
        if let Some(project) = project.filter(|project| !project.is_empty()) {
            detail = if detail.is_empty() {
                project
            } else {
                format!("{detail} · {project}")
            };
        }
        let blocker = employee.and_then(|employee| employee.blocker.clone());
        let model_detail = if let Some((model_key, provider, model, effort)) =
            self.boss_ui.queued_model_targets.get(&id)
        {
            let name = self.model_display_name_on(*model_key, *provider, Some(model));
            Some(match effort.as_deref() {
                Some(effort) => format!(
                    "{name} · {}",
                    self.reasoning_effort_label_on(*model_key, *provider, Some(model), effort)
                ),
                None => name,
            })
        } else {
            session.map(|session| self.session_sidebar_model_detail(session))
        };
        let status =
            employee.map(|employee| self.boss_employee_status_label(employee, session, &theme));
        let resume = employee.and_then(|employee| {
            self.employee_resume_button(
                key,
                id,
                employee_resume_action(employee),
                "row",
                &theme,
                cx,
            )
        });
        let stamp = employee.and_then(|employee| {
            if employee.lifecycle() == waku_protocol::boss::EmployeeLifecycle::Queued {
                employee.queued_at.or(employee.created_at)
            } else if self.boss_ui.expired.contains(&id) {
                employee.expired_at.or(employee.created_at)
            } else {
                employee.created_at
            }
        });
        let job_icon = self
            .boss_ui
            .employee_icons
            .get(&id)
            .copied()
            .flatten()
            .map(crate::custom_commands::icon_path);
        div()
            .id(SharedString::from(format!("boss-employee-row-{id}")))
            .tab_index(0)
            .h(px(42.0))
            .w_full()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| {
                this.request_session_activation(id, SessionActivationTransition::Visit, cx)
            })
            .child(self.boss_avatar_animated(&identity, 24.0, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(sp(14.0))
                            .line_height(sp(17.0))
                            .text_color(theme.text)
                            .truncate()
                            .child(identity.name.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .min_w_0()
                            .text_size(sp(13.0))
                            .line_height(sp(15.0))
                            .text_color(theme.text_tertiary)
                            .when(!detail.is_empty(), |row| {
                                row.child(icon(
                                    job_icon.unwrap_or_else(|| job_title_icon(&detail)),
                                    12.0,
                                    theme.text_tertiary,
                                ))
                            })
                            .child(div().min_w_0().truncate().child(detail))
                            .when_some(blocker, |row, blocker| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .text_color(theme.warning)
                                        .truncate()
                                        .child(format!("— {blocker}")),
                                )
                            }),
                    ),
            )
            .when_some(model_detail, |row, model| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(sp(12.0))
                        .text_color(theme.text_tertiary)
                        .child(model),
                )
            })
            .when_some(status, |row, (label, color)| {
                row.child(
                    div()
                        .flex_none()
                        .max_w(px(280.0))
                        .truncate()
                        .text_size(sp(12.0))
                        .text_color(color)
                        .child(label),
                )
            })
            .when_some(resume, |row, button| row.child(button))
            .when_some(stamp, |row, stamp| {
                row.child(
                    div()
                        .flex_none()
                        .w(px(72.0))
                        .text_right()
                        .text_size(sp(12.0))
                        .text_color(theme.text_ghost)
                        .child(sidebar::format_time_ago(unix_time().saturating_sub(stamp))),
                )
            })
            .into_any_element()
    }

    /// The Employees row's readable status — lifecycle first, then the
    /// live session's verdict. Finished, failed, cancelled, and expired
    /// stay distinct: expiry describes the roster record, not the outcome.
    fn boss_employee_status_label(
        &self,
        employee: &waku_protocol::boss::BossEmployee,
        session: Option<&AgentSession>,
        theme: &Theme,
    ) -> (String, Hsla) {
        use waku_protocol::boss::EmployeeLifecycle;
        if employee.workspace_transition && !employee.expired {
            return (tr!("boss.status_switching_workspace"), theme.text_secondary);
        }
        match employee.lifecycle() {
            EmployeeLifecycle::Queued => {
                let detail = self
                    .boss_ui
                    .queued
                    .get(&employee.session_id)
                    .cloned()
                    .unwrap_or_else(|| tr!("boss.goals_queue_admission"));
                (
                    format!("{} · {detail}", tr!("boss.goals_status_queued")),
                    theme.text_secondary,
                )
            }
            EmployeeLifecycle::Dispatching => {
                (tr!("boss.goals_status_starting"), theme.text_secondary)
            }
            EmployeeLifecycle::Finishing => {
                (tr!("boss.goals_status_finishing"), theme.text_secondary)
            }
            EmployeeLifecycle::Working => {
                if employee.blocker.is_some() {
                    (tr!("boss.goals_status_blocked"), theme.warning)
                } else {
                    match session.map(|session| session.status) {
                        Some(SessionStatus::Working | SessionStatus::Connecting) => (
                            tr!("sidebar.status_working"),
                            status_color(theme, SessionStatus::Working),
                        ),
                        Some(SessionStatus::Waiting) => (
                            tr!("sidebar.status_waiting"),
                            status_color(theme, SessionStatus::Waiting),
                        ),
                        Some(SessionStatus::Background) => (
                            tr!("sidebar.status_background"),
                            status_color(theme, SessionStatus::Background),
                        ),
                        Some(SessionStatus::Failed) => (tr!("sidebar.status_failed"), theme.danger),
                        Some(SessionStatus::Idle) => (tr!("boss.status_idle"), theme.text_tertiary),
                        None => (tr!("boss.goals_status_starting"), theme.text_secondary),
                    }
                }
            }
            EmployeeLifecycle::Expired => {
                if employee.cancelled {
                    (tr!("boss.status_cancelled"), theme.text_tertiary)
                } else if session.is_some_and(|session| session.status == SessionStatus::Failed) {
                    (tr!("sidebar.status_failed"), theme.danger)
                } else if employee.blocker.is_some() {
                    (tr!("boss.goals_status_blocked"), theme.warning)
                } else if session.is_none() {
                    (tr!("boss.status_expired"), theme.text_tertiary)
                } else {
                    (tr!("boss.employee_finished"), theme.text_tertiary)
                }
            }
        }
    }

    /// The roster record a Resume targets — live-roster employees and
    /// the retired entries their transcripts still resolve to, paired
    /// with the daemon that owns the record.
    fn boss_employee_record(
        &self,
        session_id: Uuid,
    ) -> Option<(DaemonKey, &waku_protocol::boss::BossEmployee)> {
        self.boss_ui.states.iter().find_map(|(key, state)| {
            state
                .employees
                .iter()
                .chain(state.retired_employees.iter())
                .find(|employee| employee.session_id == session_id)
                .map(|employee| (*key, employee))
        })
    }

    /// The Resume control's dispatch — the same `resume` operation the
    /// boss issues, fired by the human directly. While another boss
    /// request holds the pipe `boss_request` drops non-Open operations,
    /// so the click reports the wait instead of going silent.
    fn resume_boss_employee(&mut self, key: DaemonKey, session_id: Uuid, cx: &mut Context<Self>) {
        if self.boss_ui.pending {
            self.show_toast(tr!("boss.loading"));
            cx.notify();
            return;
        }
        self.boss_request(
            key,
            BossOperation::Resume { session_id },
            BossReply::Resume,
            cx,
        );
    }

    /// The Resume chip an expired employee's surfaces carry — its chat
    /// page's header and its Employees-list row. `slot` disambiguates
    /// the element id when both render at once. A disabled control stays
    /// in the tab order so the reason is still reachable.
    fn employee_resume_button(
        &self,
        key: DaemonKey,
        session_id: Uuid,
        action: EmployeeResume,
        slot: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if action == EmployeeResume::Hidden {
            return None;
        }
        let enabled = action == EmployeeResume::Enabled;
        let label = tr!("boss.resume_employee");
        let button = div()
            .id(SharedString::from(format!(
                "employee-resume-{slot}-{session_id}"
            )))
            .tab_index(0)
            .h(px(22.0))
            .px(px(7.0))
            .rounded(px(8.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .bg(theme.overlay)
            .text_size(sp(12.5))
            .aria_label(label.clone())
            .tooltip(Tooltip::text(if enabled {
                tr!("boss.resume_employee_hint")
            } else {
                tr!("boss.resume_unavailable")
            }))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            // The header's drag region and the list row's activation both
            // answer the same press without this guard.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(icon("icons/play.svg", 11.0, theme.text_tertiary))
            .child(label);
        let button = if enabled {
            button
                .cursor_pointer()
                .text_color(theme.text_secondary)
                .hover(|style| style.bg(theme.overlay_strong))
                .on_activation(cx, move |this, _, cx| {
                    this.resume_boss_employee(key, session_id, cx)
                })
        } else {
            button
                .cursor_default()
                .text_color(theme.text_tertiary)
                .opacity(0.55)
        };
        Some(button.into_any_element())
    }

    /// The ellipsis popover an employee's top bar carries after its job
    /// title — the brief, persona, memory, and resource inventory the
    /// summon recorded. The boss's own chat gets no button: it has no
    /// assignment to show.
    fn employee_assignment_popover(
        &self,
        session_id: Uuid,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let session = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)?;
        if !self.session_is_employee(session) {
            return None;
        }
        let (state, employee) = self.boss_ui.states.values().find_map(|state| {
            state
                .employees
                .iter()
                .chain(state.retired_employees.iter())
                .find(|employee| employee.session_id == session_id)
                .map(|employee| (state, employee))
        })?;
        let rows = Rc::new(self.employee_assignment_rows(session, state, employee));
        let handle = self.menu_handle(format!("employee-assignment-{session_id}"), cx);
        let trigger = div()
            .id(SharedString::from(format!(
                "employee-assignment-trigger-{session_id}"
            )))
            .size(px(22.0))
            .rounded(px(7.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .hover(|style| style.bg(theme.overlay))
            .when(handle.is_open(), |style| style.bg(theme.overlay_strong))
            .tooltip(Tooltip::text(tr!("boss.assignment")))
            .child(icon(
                "icons/ellipsis-vertical.svg",
                14.0,
                theme.text_tertiary,
            ));
        Some(popover(
            trigger,
            &handle,
            MenuAlign::BelowRight,
            move |handle, _, cx| {
                let theme = Theme::current(cx);
                let mut content = div()
                    .id(SharedString::from(format!(
                        "employee-assignment-scroll-{session_id}"
                    )))
                    .max_h(px(420.0))
                    .overflow_y_scroll()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(30.0))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .text_size(sp(13.5))
                            .text_color(theme.text_tertiary)
                            .child(tr!("boss.assignment")),
                    );
                for (label, value) in rows.iter() {
                    content = content.child(
                        div()
                            .w_full()
                            .px(px(8.0))
                            .pb(px(6.0))
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(label.clone()),
                            )
                            .child(
                                div()
                                    .text_size(sp(12.5))
                                    .text_color(theme.text_secondary)
                                    .child(value.clone()),
                            ),
                    );
                }
                div()
                    .id(SharedString::from(format!(
                        "employee-assignment-card-{session_id}"
                    )))
                    .track_focus(handle.focus_handle())
                    .w(px(300.0))
                    .rounded(px(15.0))
                    .border(hairline())
                    .border_color(theme.border_subtle)
                    .overflow_hidden()
                    .bg(theme.raised)
                    .shadow_lg()
                    .child(content)
                    .into_any_element()
            },
        ))
    }

    /// The inventory the employee top bar's assignment popover lists —
    /// label/value pairs for the brief, persona, model, workspace,
    /// access, and resource state the summon recorded.
    fn employee_assignment_rows(
        &self,
        session: &AgentSession,
        state: &BossState,
        employee: &waku_protocol::boss::BossEmployee,
    ) -> Vec<(String, String)> {
        // The assigned role composes over the canonical Employee base —
        // the popover names both so "Employee + Researcher" reads as
        // layered rather than replaced.
        let base_name = state
            .employee_persona_id
            .and_then(|id| state.personas.iter().find(|persona| persona.id == id))
            .map(|persona| persona.name.clone());
        let role_name = state
            .personas
            .iter()
            .find(|persona| persona.id == employee.persona_id)
            .filter(|persona| Some(persona.id) != state.employee_persona_id)
            .map(|persona| persona.name.clone());
        let persona_name = match (base_name, role_name) {
            (Some(base), Some(role)) => Some(tr!(
                "boss.assignment_persona_layered",
                base = base,
                role = role
            )),
            (Some(base), None) => {
                if Some(employee.persona_id) == state.employee_persona_id {
                    Some(base)
                } else if state.persona_id == employee.persona_id {
                    Some(tr!("boss.assignment_persona_invalid", base = base))
                } else {
                    Some(tr!("boss.assignment_persona_missing", base = base))
                }
            }
            (None, role) => role,
        };
        let workspace = match &session.workspace {
            SessionWorkspace::Local => tr!("boss.assignment_workspace_local"),
            SessionWorkspace::NewWorktree { .. } => tr!("boss.assignment_workspace_new"),
            SessionWorkspace::Worktree { name, path, .. } => {
                if name.is_empty() {
                    path.display().to_string()
                } else {
                    name.clone()
                }
            }
        };
        let permissions = &employee.permissions;
        let mut access = Vec::new();
        if !permissions.bucket_ids.is_empty() {
            access.push(tr!(
                "boss.assignment_buckets",
                count = permissions.bucket_ids.len()
            ));
        }
        if !permissions.integration_ids.is_empty() {
            access.push(tr!(
                "boss.assignment_integrations",
                count = permissions.integration_ids.len()
            ));
        }
        if permissions.summon_employees {
            access.push(tr!("boss.delegate"));
        }
        if permissions.computer_use {
            access.push(tr!("boss.computer"));
        }
        let resources = employee.ticket.as_ref().map(|ticket| {
            let resources = &ticket.resources;
            if resources.is_empty() {
                tr!("boss.assignment_resources_none")
            } else {
                let mut parts = Vec::new();
                if !resources.exclusive.is_empty() {
                    parts.push(tr!(
                        "boss.resource_exclusive",
                        count = resources.exclusive.len()
                    ));
                }
                if resources.resident_devices > 0 {
                    parts.push(tr!(
                        "boss.resource_devices",
                        count = resources.resident_devices
                    ));
                }
                if resources.native_builds > 0 {
                    parts.push(tr!("boss.resource_builds", count = resources.native_builds));
                }
                if resources.desktop_input > 0 {
                    parts.push(tr!("boss.resource_desktop"));
                }
                if ticket.reservation.is_some() {
                    tr!(
                        "boss.assignment_resources_reserved",
                        detail = parts.join(" · ")
                    )
                } else {
                    tr!(
                        "boss.assignment_resources_requested",
                        detail = parts.join(" · ")
                    )
                }
            }
        });
        let brief = employee
            .ticket
            .as_ref()
            .map(|ticket| ticket.prompt.trim().to_owned())
            .filter(|prompt| !prompt.is_empty());
        let model = self.session_sidebar_model_detail(session);
        [
            brief.map(|brief| (tr!("boss.assignment_brief"), brief)),
            persona_name.map(|name| (tr!("boss.assignment_persona"), name)),
            Some((tr!("boss.assignment_model"), model)),
            Some((tr!("boss.assignment_workspace"), workspace)),
            Some((
                tr!("boss.assignment_access"),
                if access.is_empty() {
                    tr!("boss.assignment_access_none")
                } else {
                    access.join(" · ")
                },
            )),
            resources.map(|line| (tr!("boss.assignment_resources"), line)),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    // ── Personas ─────────────────────────────────────────────────────────

    /// Personas read first: the boss's own persona sits above the reusable
    /// roles in the list column, and the detail pane shows readable
    /// instructions plus quiet grant metadata. Editing is a deliberate
    /// mode behind the detail's Edit action and the list's New persona.
    fn render_boss_personas_section(
        &mut self,
        key: DaemonKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        if self.boss_ui.persona_search.is_none() {
            let input = cx.new(|cx| {
                TextInput::new(window, cx)
                    .tab_index(0)
                    .accessibility_label(tr!("boss.persona_search"))
            });
            cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Edited) {
                    this.boss_ui.persona_query = input.read(cx).content().to_owned();
                    cx.notify();
                }
            })
            .detach();
            self.boss_ui.persona_search = Some(input);
        }
        let search = self.boss_ui.persona_search.clone();
        let Some(state) = self.boss_ui.states.get(&key).cloned() else {
            return boss_empty_state(
                &theme,
                "icons/user-round.svg",
                tr!("boss.personas_empty"),
                tr!("boss.personas_empty_hint"),
            )
            .into_any_element();
        };
        let query = self.boss_ui.persona_query.trim().to_lowercase();
        let boss_persona = state
            .personas
            .iter()
            .find(|persona| persona.id == state.persona_id)
            .filter(|persona| query.is_empty() || persona.name.to_lowercase().contains(&query))
            .cloned();
        // The detail pane never sits empty while roles exist: the stored
        // selection wins when visible, the first visible row otherwise —
        // the Skills page's rule.
        let visible: Vec<Uuid> = self
            .boss_ui
            .rows
            .iter()
            .filter_map(|row| match row {
                BossItem::Persona(id) => Some(*id),
                _ => None,
            })
            .collect();
        let selected = self
            .boss_ui
            .personas_selected
            .get(&key)
            .copied()
            .filter(|id| visible.contains(id) || boss_persona.as_ref().is_some_and(|p| p.id == *id))
            .or_else(|| {
                visible
                    .first()
                    .copied()
                    .or_else(|| boss_persona.as_ref().map(|persona| persona.id))
            });
        if let Some(id) = selected {
            self.boss_ui.personas_selected.insert(key, id);
        }
        let mut list_column = div()
            .w(px(300.0))
            .flex_none()
            .flex()
            .flex_col()
            .min_h_0()
            .border_r_1()
            .border_color(theme.separator)
            .child(
                div()
                    .flex_none()
                    .px(px(10.0))
                    .pt(px(10.0))
                    .pb(px(8.0))
                    .children(search.map(|input| {
                        TextField::new("boss-persona-search", input)
                            .icon("icons/search.svg", 13.0)
                            .w_full()
                    })),
            );
        let has_boss_persona = boss_persona.is_some();
        if let Some(ref persona) = boss_persona {
            list_column = list_column
                .child(boss_section_label(&theme, tr!("boss.section_boss"), true))
                .child(self.render_boss_persona_row(key, persona, true, selected, &theme, cx));
        }
        if !visible.is_empty() || !has_boss_persona {
            list_column = list_column.child(boss_section_label(
                &theme,
                tr!("boss.section_roles"),
                has_boss_persona,
            ));
        }
        if state.employee_persona_id.is_none() {
            // The canonical Employee base is awaiting the human's pick —
            // the choice lives on each candidate's detail.
            list_column = list_column.child(
                div()
                    .flex_none()
                    .px(px(12.0))
                    .pb(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(icon("icons/info.svg", 11.0, theme.info))
                    .child(
                        div()
                            .text_size(sp(11.5))
                            .text_color(theme.info)
                            .child(tr!("boss.base_choice_hint")),
                    ),
            );
        }
        if self.boss_ui.rows.is_empty() {
            list_column = list_column.child(
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(if query.is_empty() {
                        tr!("boss.roles_empty")
                    } else {
                        tr!("boss.roles_no_match")
                    }),
            );
        } else {
            list_column = list_column.child(self.boss_item_list(cx));
        }
        list_column = list_column.child(
            div()
                .flex_none()
                .px(px(10.0))
                .py(px(8.0))
                .border_t_1()
                .border_color(theme.separator)
                .child(
                    boss_button("boss-new-persona", tr!("boss.new_persona"), &theme)
                        .child(icon("icons/plus.svg", 13.0, theme.text_secondary))
                        .child(tr!("boss.new_persona"))
                        .on_activation(cx, move |this, window, cx| {
                            this.edit_boss_persona(key, None, window, cx)
                        }),
                ),
        );
        let detail = if self
            .boss_ui
            .editor
            .as_ref()
            .is_some_and(|editor| editor.key == key)
        {
            self.render_boss_editor(cx)
        } else {
            match selected.and_then(|id| {
                state
                    .personas
                    .iter()
                    .find(|persona| persona.id == id)
                    .cloned()
            }) {
                Some(persona) => {
                    let is_boss = persona.id == state.persona_id;
                    self.render_boss_persona_detail(key, &persona, is_boss, &state, cx)
                }
                None => boss_detail_placeholder(
                    &theme,
                    "icons/user-round.svg",
                    tr!("boss.persona_select"),
                ),
            }
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(list_column)
            .child(div().flex_1().min_w_0().flex().flex_col().child(detail))
            .into_any_element()
    }

    /// A persona row in the list column: icon, name, and the first line of
    /// its instructions as the short purpose.
    fn render_boss_persona_row(
        &self,
        key: DaemonKey,
        persona: &BossPersona,
        boss_role: bool,
        selected: Option<Uuid>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = persona.id;
        let selected = selected == Some(id);
        let icon_path = persona
            .icon
            .map(crate::custom_commands::icon_path)
            .unwrap_or("icons/user-round.svg");
        let purpose = persona
            .markdown
            .lines()
            .map(|line| line.trim().trim_start_matches('#').trim())
            .find(|line| !line.is_empty())
            .unwrap_or_default()
            .to_owned();
        div()
            .id(SharedString::from(format!("boss-persona-{id}")))
            .tab_index(0)
            .h(px(42.0))
            .w_full()
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_pointer()
            .when(selected, |row| row.bg(theme.overlay))
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| {
                if this.boss_editor_dirty(cx) {
                    this.show_toast(tr!("boss.save_first"));
                } else {
                    if this
                        .boss_ui
                        .editor
                        .as_ref()
                        .is_some_and(|editor| editor.key == key && editor.persona != id)
                    {
                        this.boss_ui.editor = None;
                    }
                    this.boss_ui.personas_selected.insert(key, id);
                }
                cx.notify();
            })
            .child(icon(icon_path, 15.0, theme.text_secondary))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(sp(14.0))
                            .line_height(sp(17.0))
                            .text_color(theme.text)
                            .truncate()
                            .child(persona.name.clone()),
                    )
                    .when(!purpose.is_empty(), |column| {
                        column.child(
                            div()
                                .text_size(sp(12.0))
                                .line_height(sp(15.0))
                                .text_color(theme.text_tertiary)
                                .truncate()
                                .child(purpose),
                        )
                    }),
            )
            .when(boss_role, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(sp(11.0))
                        .text_color(theme.text_tertiary)
                        .child(tr!("boss.role_boss")),
                )
            })
            .when(
                self.boss_ui
                    .states
                    .get(&key)
                    .is_some_and(|state| state.employee_persona_id == Some(persona.id)),
                |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_size(sp(11.0))
                            .text_color(theme.text_tertiary)
                            .child(tr!("boss.role_base")),
                    )
                },
            )
            .when(
                self.boss_ui.states.get(&key).is_some_and(|state| {
                    state.persona_default_role(persona.id).is_some_and(|role| {
                        role != PersonaDefaultRole::Boss && role != PersonaDefaultRole::Employee
                    })
                }),
                |row| {
                    // A canonical specialist's marker — a custom persona
                    // can share its name but never carries this chip.
                    row.child(
                        div()
                            .flex_none()
                            .text_size(sp(11.0))
                            .text_color(theme.text_tertiary)
                            .child(tr!("boss.role_default")),
                    )
                },
            )
            .when(
                self.boss_ui.states.get(&key).is_some_and(|state| {
                    state.employee_persona_id.is_none()
                        && state.persona_default_role(persona.id).is_none()
                }),
                |row| {
                    // The Employee base is awaiting the human's choice —
                    // every candidate gets the same quiet marker.
                    row.child(
                        div()
                            .flex_none()
                            .text_size(sp(11.0))
                            .text_color(theme.info)
                            .child(tr!("boss.base_choice_short")),
                    )
                },
            )
            .when(
                self.boss_ui.states.get(&key).is_some_and(|state| {
                    state
                        .persona_default_role(persona.id)
                        .is_some_and(|role| {
                            state.persona_default_notice.as_ref().is_some_and(|notice| {
                                notice
                                    .updates
                                    .iter()
                                    .any(|update| update.role == role && !update.adopted)
                            })
                        })
                }),
                |row| {
                    // The quiet upgrade indicator — text, not color alone.
                    row.child(
                        div()
                            .flex_none()
                            .text_size(sp(11.0))
                            .text_color(theme.info)
                            .child(tr!("boss.default_update_short")),
                    )
                },
            )
            .into_any_element()
    }

    /// The read-first persona detail: identity header, grant metadata as
    /// label/value rows, a deliberate Edit action, then the rendered
    /// instructions.
    fn render_boss_persona_detail(
        &mut self,
        key: DaemonKey,
        persona: &BossPersona,
        boss_role: bool,
        state: &BossState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let icon_path = persona
            .icon
            .map(crate::custom_commands::icon_path)
            .unwrap_or("icons/user-round.svg");
        let permissions = &persona.permissions;
        let bucket_line = if permissions.bucket_ids.is_empty() {
            tr!("boss.detail_none")
        } else {
            permissions.bucket_ids.join(", ")
        };
        let pinned_line = if persona.pinned_files.is_empty() {
            tr!("boss.detail_none")
        } else {
            persona.pinned_files.join(", ")
        };
        let integration_line = if permissions.integration_ids.is_empty() {
            tr!("boss.detail_none")
        } else {
            permissions.integration_ids.join(", ")
        };
        let yes_no = |yes: bool| {
            if yes {
                tr!("boss.detail_yes")
            } else {
                tr!("boss.detail_no")
            }
        };
        let rows: Vec<(String, AnyElement)> = vec![
            (
                tr!("boss.detail_buckets"),
                boss_plain_value(&theme, bucket_line),
            ),
            (
                tr!("boss.detail_pinned"),
                boss_plain_value(&theme, pinned_line),
            ),
            (
                tr!("boss.detail_integrations"),
                boss_plain_value(&theme, integration_line),
            ),
            (
                tr!("boss.detail_summon"),
                boss_plain_value(&theme, yes_no(permissions.summon_employees)),
            ),
            (
                tr!("boss.detail_computer"),
                boss_plain_value(&theme, yes_no(permissions.computer_use)),
            ),
        ];
        let count = rows.len();
        let mut info = div().mt(px(14.0)).flex().flex_col();
        for (index, (label, value)) in rows.into_iter().enumerate() {
            info = info.child(boss_info_row(&theme, label, value, index + 1 == count));
        }
        let palette = MarkdownPalette::from_theme(&theme);
        let document: Option<AnyElement> = (!persona.markdown.trim().is_empty()).then(|| {
            let mut cache = self.boss_ui.persona_markdown.borrow_mut();
            if !matches!(cache.as_ref(), Some((cached, _)) if *cached == persona.id) {
                *cache = Some((persona.id, MarkdownView::document()));
            }
            let (_, view) = cache.as_mut().expect("entry ensured above");
            view.set_text(&persona.markdown, false);
            let ctx = MarkdownCtx::new(
                format!("persona-md-{}", persona.id),
                &palette,
                MarkdownMetrics::document(self.state.ui_font_size, self.state.code_font_size),
                self.boss_ui.persona_selection.clone(),
            )
            .with_families(crate::fonts::current(cx))
            .with_math_enabled(self.state.render_math)
            .with_guided_reading(self.guided_reading())
            .with_link_items(self.markdown_link_menu_items.clone())
            .with_link_handler(self.markdown_link_handler.clone())
            .with_standalone_context_menu(self.menu_handle("persona-detail-math", cx));
            div()
                .mt(px(16.0))
                .pt(px(14.0))
                .border_t_1()
                .border_color(theme.separator)
                .child(
                    div()
                        .text_size(sp(12.5))
                        .text_color(theme.text_ghost)
                        .child(tr!("boss.instructions")),
                )
                .child(
                    div()
                        .mt(px(10.0))
                        .text_color(theme.text)
                        .children(md::render::markdown(view, &ctx)),
                )
                .into_any_element()
        });
        let selection_input = {
            let selection = self.boss_ui.persona_selection.clone();
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal).id,
                move |_, region, window, _| {
                    md::render::install_selection_input(region, window, &selection, None)
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };
        let persona_for_edit = persona.clone();
        let default_role = state.persona_default_role(persona.id);
        let default_card = default_role
            .map(|role| self.render_persona_default_card(key, persona, role, state, cx));
        // While the canonical Employee base is unassigned, every saved
        // non-Boss role is a candidate and offers the human's pick —
        // the marker write keeps the record's content and grants intact.
        let base_choice_card = (state.employee_persona_id.is_none() && default_role.is_none())
            .then(|| self.render_employee_base_choice_card(key, persona, cx));
        // A custom or specialist role composes over the canonical
        // Employee base — the detail shows the inherited text read-only
        // so the layering is visible without pretending it is editable
        // here.
        let base_document: Option<AnyElement> = (!matches!(
            default_role,
            Some(PersonaDefaultRole::Boss | PersonaDefaultRole::Employee)
        ))
            .then(|| {
                state
                    .personas
                    .iter()
                    .find(|persona| Some(persona.id) == state.employee_persona_id)
            })
            .flatten()
            .filter(|base| !base.markdown.trim().is_empty())
            .map(|base| {
                let mut cache = self.boss_ui.persona_base_markdown.borrow_mut();
                if !matches!(cache.as_ref(), Some((cached, _)) if *cached == persona.id) {
                    *cache = Some((persona.id, MarkdownView::document()));
                }
                let (_, view) = cache.as_mut().expect("entry ensured above");
                view.set_text(&base.markdown, false);
                let ctx = MarkdownCtx::new(
                    format!("persona-base-md-{}", persona.id),
                    &palette,
                    MarkdownMetrics::document(self.state.ui_font_size, self.state.code_font_size),
                    self.boss_ui.persona_selection.clone(),
                )
                .with_families(crate::fonts::current(cx))
                .with_math_enabled(self.state.render_math)
                .with_guided_reading(self.guided_reading())
                .with_link_items(self.markdown_link_menu_items.clone())
                .with_link_handler(self.markdown_link_handler.clone())
                .with_standalone_context_menu(self.menu_handle("persona-base-math", cx));
                div()
                    .mt(px(16.0))
                    .pt(px(14.0))
                    .border_t_1()
                    .border_color(theme.separator)
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .text_color(theme.text_ghost)
                            .child(tr!("boss.persona_base_section")),
                    )
                    .child(
                        div()
                            .mt(px(10.0))
                            .text_color(theme.text_secondary)
                            .children(md::render::markdown(view, &ctx)),
                    )
                    .into_any_element()
            });
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                div()
                    .id("boss-persona-detail-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.boss_ui.persona_scroll)
                    .px(px(24.0))
                    .pt(px(18.0))
                    .pb(px(20.0))
                    .child(md::render::frame_reset(
                        self.boss_ui.persona_selection.clone(),
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .w(px(38.0))
                                    .h(px(38.0))
                                    .flex_none()
                                    .rounded(px(11.0))
                                    .bg(theme.overlay)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(icon_path, 18.0, theme.text_secondary)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(sp(15.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(persona.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .mt(px(2.0))
                                            .text_size(sp(12.5))
                                            .text_color(theme.text_tertiary)
                                            .child(if boss_role {
                                                tr!("boss.role_boss")
                                            } else if state.employee_persona_id == Some(persona.id)
                                            {
                                                tr!("boss.role_employee_base")
                                            } else if default_role.is_some() {
                                                tr!("boss.role_employee_shipped")
                                            } else {
                                                tr!("boss.role_employee_custom")
                                            }),
                                    ),
                            )
                            .child(
                                boss_button("boss-edit-persona", tr!("boss.edit"), &theme)
                                    .child(icon("icons/pencil.svg", 13.0, theme.text_secondary))
                                    .child(tr!("boss.edit"))
                                    .on_activation(cx, move |this, window, cx| {
                                        this.edit_boss_persona(
                                            key,
                                            Some(persona_for_edit.clone()),
                                            window,
                                            cx,
                                        )
                                    }),
                            ),
                    )
                    .child(info)
                    .children(default_card)
                    .children(base_choice_card)
                    .children(document)
                    .children(base_document),
            )
            .child(scrollbar::vertical(
                &self.boss_ui.persona_scroll,
                &self.boss_ui.persona_scrollbar,
            ))
            .child(selection_input)
            .into_any_element()
    }

    /// The pending Employee-base decision on a candidate's detail — the
    /// reconciler could not tell which saved persona is the canonical
    /// base, so the choice waits on the human. Picking a card records the
    /// marker; the role's instructions, permissions, icon, and pinned
    /// documents stay exactly as saved.
    fn render_employee_base_choice_card(
        &mut self,
        key: DaemonKey,
        persona: &BossPersona,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let persona_id = persona.id;
        div()
            .mt(px(14.0))
            .w_full()
            .rounded(px(10.0))
            .border(hairline())
            .border_color(theme.border)
            .px(px(12.0))
            .py(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(icon("icons/info.svg", 11.0, theme.info))
                    .child(
                        div()
                            .text_size(sp(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_tertiary)
                            .child(tr!("boss.base_choice_title")),
                    ),
            )
            .child(
                div()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("boss.base_choice_body")),
            )
            .child(
                div().flex().gap(px(8.0)).child(
                    boss_button("boss-base-choice", tr!("boss.base_choice_action"), &theme)
                        .child(icon("icons/check.svg", 12.0, theme.text_secondary))
                        .child(tr!("boss.base_choice_action"))
                        .on_activation(cx, move |this, _, cx| {
                            this.boss_request(
                                key,
                                BossOperation::PersonaDefault {
                                    action: PersonaDefaultAction::ChooseEmployeeBase { persona_id },
                                },
                                BossReply::BaseChoice,
                                cx,
                            );
                        }),
                ),
            )
            .into_any_element()
    }

    /// The shipped-default card on a canonical persona's detail: status
    /// and revision labels, the quiet update indicator, and the inspect /
    /// compare / reset / undo / review controls. Reset runs behind a
    /// preview that names the instruction-only scope and the complete
    /// replacement; proposals and stale baselines surface their state
    /// instead of overwriting intervening edits.
    fn render_persona_default_card(
        &mut self,
        key: DaemonKey,
        persona: &BossPersona,
        role: PersonaDefaultRole,
        state: &BossState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let shipped = shipped_persona_default(role);
        let defaults = state.persona_defaults.get(role);
        let persona_id = persona.id;
        let pane = self
            .boss_ui
            .persona_default_pane
            .get(&persona_id)
            .copied()
            .unwrap_or_default();
        let using_latest = persona.markdown == shipped.markdown;
        let untouched = shipped_persona_revision(role, &persona.markdown);
        let update_pending = state.persona_default_notice.as_ref().is_some_and(|notice| {
            notice
                .updates
                .iter()
                .any(|update| update.role == role && !update.adopted)
        });
        let undo = defaults.undo.clone();
        let undo_applies = undo
            .as_ref()
            .is_some_and(|undo| undo.applied_markdown == persona.markdown);
        let proposal = defaults.proposal.clone();
        let proposal_stale = proposal
            .as_ref()
            .is_some_and(|proposal| proposal.baseline_markdown != persona.markdown);
        let status = if using_latest {
            tr!("boss.default_latest", revision = shipped.revision)
        } else if let Some(revision) = untouched {
            tr!("boss.default_untouched", revision = revision)
        } else if let Some(revision) = defaults.starting_revision {
            tr!("boss.default_customized", revision = revision)
        } else {
            tr!("boss.default_unknown")
        };
        let reviewed_latest = !using_latest && defaults.reviewed_revision == Some(shipped.revision);
        let diff_text = (pane.compare || pane.reset_preview).then(|| {
            let fingerprint = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                persona.markdown.hash(&mut hasher);
                shipped.revision.hash(&mut hasher);
                hasher.finish()
            };
            let mut cache = self.boss_ui.persona_diff.borrow_mut();
            let stale = !matches!(
                cache.as_ref(),
                Some((id, seen, _)) if *id == persona_id && *seen == fingerprint
            );
            if stale {
                *cache = Some((
                    persona_id,
                    fingerprint,
                    instruction_diff(&persona.markdown, shipped.markdown).unwrap_or_default(),
                ));
            }
            cache
                .as_ref()
                .map(|(_, _, text)| text.clone())
                .unwrap_or_default()
        });
        let diff_block = diff_text.filter(|text| !text.is_empty()).map(|text| {
            let code = crate::fonts::current(cx).code;
            let mut block = div()
                .w_full()
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border)
                .px(px(10.0))
                .py(px(8.0))
                .flex()
                .flex_col();
            for line in text.lines() {
                let color = if line.starts_with('+') {
                    theme.success
                } else if line.starts_with('-') {
                    theme.danger
                } else if line.starts_with('@') {
                    theme.text_tertiary
                } else {
                    theme.text_secondary
                };
                block = block.child(
                    div()
                        .text_size(sp(11.5))
                        .font(font(code.clone()))
                        .text_color(color)
                        .child(line.to_owned()),
                );
            }
            block
        });
        let shipped_block = (pane.shipped || pane.reset_preview).then(|| {
            let mut block = div()
                .w_full()
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border)
                .px(px(10.0))
                .py(px(8.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(sp(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_tertiary)
                        .child(tr!(
                            "boss.default_shipped_label",
                            revision = shipped.revision
                        )),
                );
            for line in shipped.markdown.lines() {
                block = block.child(
                    div()
                        .mt(px(4.0))
                        .text_size(sp(12.0))
                        .text_color(theme.text_secondary)
                        .child(line.to_owned()),
                );
            }
            block
        });
        let proposal_block = proposal.map(|proposal| {
            let mut block = div()
                .w_full()
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border)
                .px(px(10.0))
                .py(px(8.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(sp(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_tertiary)
                        .child(tr!(
                            "boss.default_proposal",
                            revision = proposal.target_revision
                        )),
                )
                .when(proposal_stale, |block| {
                    block.child(
                        div()
                            .mt(px(4.0))
                            .text_size(sp(12.0))
                            .text_color(theme.warning)
                            .child(tr!("boss.default_proposal_stale")),
                    )
                });
            for line in proposal.markdown.lines() {
                block = block.child(
                    div()
                        .mt(px(4.0))
                        .text_size(sp(12.0))
                        .text_color(theme.text_secondary)
                        .child(line.to_owned()),
                );
            }
            let saved = persona.markdown.clone();
            let approved = proposal.markdown.clone();
            block
                .child(
                    div()
                        .mt(px(8.0))
                        .flex()
                        .gap(px(8.0))
                        .when(!proposal_stale, |row| {
                            row.child(
                                boss_button(
                                    "boss-default-adopt",
                                    tr!("boss.default_adopt"),
                                    &theme,
                                )
                                .child(tr!("boss.default_adopt"))
                                .on_activation(
                                    cx,
                                    move |this, _, cx| {
                                        this.boss_request(
                                            key,
                                            BossOperation::PersonaDefault {
                                                action: PersonaDefaultAction::Adopt {
                                                    role,
                                                    markdown: approved.clone(),
                                                    expected_saved: Some(saved.clone()),
                                                },
                                            },
                                            BossReply::PersonaDefault,
                                            cx,
                                        );
                                    },
                                ),
                            )
                        })
                        .child(
                            boss_button(
                                "boss-default-dismiss",
                                tr!("boss.default_dismiss_proposal"),
                                &theme,
                            )
                            .child(tr!("boss.default_dismiss_proposal"))
                            .on_activation(cx, move |this, _, cx| {
                                this.boss_request(
                                    key,
                                    BossOperation::PersonaDefault {
                                        action: PersonaDefaultAction::DismissProposal { role },
                                    },
                                    BossReply::PersonaDefault,
                                    cx,
                                );
                            }),
                        ),
                )
                .into_any_element()
        });
        let mut card = div()
            .mt(px(14.0))
            .w_full()
            .rounded(px(10.0))
            .border(hairline())
            .border_color(theme.border)
            .px(px(12.0))
            .py(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(sp(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_tertiary)
                            .child(tr!("boss.default_title")),
                    )
                    .when(update_pending, |row| {
                        row.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .child(icon("icons/info.svg", 11.0, theme.info))
                                .child(div().text_size(sp(11.5)).text_color(theme.info).child(
                                    tr!("boss.default_update", revision = shipped.revision),
                                )),
                        )
                    }),
            );
        if pane.reset_preview {
            card = card
                .child(
                    div()
                        .text_size(sp(12.5))
                        .text_color(theme.text)
                        .child(tr!("boss.default_reset_title", name = persona.name.clone())),
                )
                .child(
                    div()
                        .text_size(sp(12.0))
                        .text_color(theme.text_secondary)
                        .child(tr!("boss.default_reset_scope")),
                )
                .when(!using_latest, |card| {
                    card.child(
                        div()
                            .text_size(sp(12.0))
                            .text_color(theme.warning)
                            .child(tr!("boss.default_reset_custom")),
                    )
                })
                .child(
                    div()
                        .text_size(sp(12.0))
                        .text_color(theme.text_secondary)
                        .child(match role {
                            PersonaDefaultRole::Boss => tr!("boss.default_timing_boss"),
                            _ => tr!("boss.default_timing_employee"),
                        }),
                )
                .children(diff_block)
                .children(shipped_block)
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            boss_button(
                                "boss-default-reset-confirm",
                                tr!("boss.default_reset"),
                                &theme,
                            )
                            .child(tr!("boss.default_reset"))
                            .on_activation(cx, move |this, _, cx| {
                                this.boss_ui
                                    .persona_default_pane
                                    .entry(persona_id)
                                    .or_default()
                                    .reset_preview = false;
                                this.boss_request(
                                    key,
                                    BossOperation::PersonaDefault {
                                        action: PersonaDefaultAction::Reset { role },
                                    },
                                    BossReply::PersonaDefault,
                                    cx,
                                );
                            }),
                        )
                        .child(
                            boss_button("boss-default-reset-cancel", tr!("common.cancel"), &theme)
                                .child(tr!("common.cancel"))
                                .on_activation(cx, move |this, _, cx| {
                                    this.boss_ui
                                        .persona_default_pane
                                        .entry(persona_id)
                                        .or_default()
                                        .reset_preview = false;
                                    cx.notify();
                                }),
                        ),
                );
            return card.into_any_element();
        }
        card = card
            .child(
                div()
                    .text_size(sp(12.5))
                    .text_color(theme.text)
                    .child(status)
                    .when(reviewed_latest, |line| {
                        line.child(SharedString::from(format!(
                            " · {}",
                            tr!("boss.default_reviewed")
                        )))
                    }),
            )
            .when(undo.is_some() && !undo_applies, |card| {
                card.child(
                    div()
                        .text_size(sp(11.5))
                        .text_color(theme.text_tertiary)
                        .child(tr!("boss.default_undo_stale")),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .child(
                        boss_button("boss-default-compare", tr!("boss.default_compare"), &theme)
                            .child(icon("icons/file-diff.svg", 12.0, theme.text_secondary))
                            .child(if update_pending {
                                tr!("boss.default_review")
                            } else {
                                tr!("boss.default_compare")
                            })
                            .on_activation(cx, move |this, _, cx| {
                                this.boss_ui
                                    .persona_default_pane
                                    .entry(persona_id)
                                    .or_default()
                                    .compare = !pane.compare;
                                cx.notify();
                            }),
                    )
                    .child(
                        boss_button(
                            "boss-default-shipped",
                            tr!("boss.default_view_shipped"),
                            &theme,
                        )
                        .child(icon("icons/sparkle.svg", 12.0, theme.text_secondary))
                        .child(tr!("boss.default_view_shipped"))
                        .on_activation(cx, move |this, _, cx| {
                            this.boss_ui
                                .persona_default_pane
                                .entry(persona_id)
                                .or_default()
                                .shipped = !pane.shipped;
                            cx.notify();
                        }),
                    )
                    .when(!using_latest, |row| {
                        row.child(
                            boss_button("boss-default-reset", tr!("boss.default_reset"), &theme)
                                .child(icon("icons/rotate-ccw.svg", 12.0, theme.text_secondary))
                                .child(tr!("boss.default_reset"))
                                .on_activation(cx, move |this, _, cx| {
                                    this.boss_ui
                                        .persona_default_pane
                                        .entry(persona_id)
                                        .or_default()
                                        .reset_preview = true;
                                    cx.notify();
                                }),
                        )
                        .child(
                            boss_button("boss-default-keep", tr!("boss.default_keep"), &theme)
                                .child(icon("icons/check.svg", 12.0, theme.text_secondary))
                                .child(tr!("boss.default_keep"))
                                .on_activation(cx, move |this, _, cx| {
                                    this.boss_request(
                                        key,
                                        BossOperation::PersonaDefault {
                                            action: PersonaDefaultAction::Keep { role },
                                        },
                                        BossReply::PersonaDefault,
                                        cx,
                                    );
                                }),
                        )
                    })
                    .when(undo_applies, |row| {
                        row.child(
                            boss_button("boss-default-undo", tr!("boss.default_undo"), &theme)
                                .child(icon("icons/rotate-ccw.svg", 12.0, theme.text_secondary))
                                .child(tr!("boss.default_undo"))
                                .on_activation(cx, move |this, _, cx| {
                                    this.boss_request(
                                        key,
                                        BossOperation::PersonaDefault {
                                            action: PersonaDefaultAction::Undo { role },
                                        },
                                        BossReply::PersonaDefault,
                                        cx,
                                    );
                                }),
                        )
                    }),
            )
            .children(diff_block)
            .children(shipped_block)
            .children(proposal_block);
        card.into_any_element()
    }

    // ── Plans ────────────────────────────────────────────────────────────

    /// One plan library: the Active/Approved/Archived filter above the
    /// same list/detail split Skills uses, with the document and its state
    /// on the right.
    fn render_boss_plans_section(&mut self, key: DaemonKey, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let filter = self
            .boss_ui
            .plans_filter
            .get(&key)
            .copied()
            .unwrap_or_default();
        let segmented = self.boss_segmented(
            "boss-plans-filter",
            vec![
                (BossPlansFilter::Active, tr!("boss.plans_active")),
                (BossPlansFilter::Approved, tr!("boss.plans_approved")),
                (BossPlansFilter::Archived, tr!("boss.plans_archived")),
            ],
            filter,
            cx,
            move |this, picked, _, cx| {
                this.boss_ui.plans_filter.insert(key, picked);
                this.sync_boss_page_rows();
                cx.notify();
            },
        );
        let visible: Vec<Uuid> = self
            .boss_ui
            .rows
            .iter()
            .filter_map(|row| match row {
                BossItem::Plan(id) => Some(*id),
                _ => None,
            })
            .collect();
        let selected = self
            .boss_ui
            .plans_selected
            .get(&key)
            .copied()
            .filter(|id| visible.contains(id))
            .or_else(|| visible.first().copied());
        if let Some(id) = selected {
            self.boss_ui.plans_selected.insert(key, id);
        }
        let list_column = div()
            .w(px(300.0))
            .flex_none()
            .flex()
            .flex_col()
            .min_h_0()
            .border_r_1()
            .border_color(theme.separator)
            .child(
                div()
                    .flex_none()
                    .px(px(10.0))
                    .pt(px(10.0))
                    .pb(px(8.0))
                    .child(segmented),
            )
            .child(if self.boss_ui.rows.is_empty() {
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(match filter {
                        BossPlansFilter::Active => tr!("boss.plans_empty_active"),
                        BossPlansFilter::Approved => tr!("boss.plans_empty_approved"),
                        BossPlansFilter::Archived => tr!("boss.plans_empty_archived"),
                    })
                    .into_any_element()
            } else {
                self.boss_item_list(cx).into_any_element()
            });
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(list_column)
            .child(self.render_boss_plan_detail(key, selected, cx))
            .into_any_element()
    }

    /// The Plans detail — the document through the same read-only markdown
    /// view the plan tab uses, headed by its state and a way back to the
    /// discussion while it is still on record.
    fn render_boss_plan_detail(
        &mut self,
        key: DaemonKey,
        selected: Option<Uuid>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let empty = |label: String| {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .child(div().text_color(theme.text_secondary).child(label))
                .into_any_element()
        };
        let Some(session_id) = selected else {
            return boss_detail_placeholder(&theme, "icons/map.svg", tr!("boss.plan_select"));
        };
        let Some(plan) = self
            .boss_ui
            .states
            .get(&key)
            .and_then(|state| {
                state
                    .planning
                    .iter()
                    .find(|plan| plan.session_id == session_id)
            })
            .cloned()
        else {
            return empty(tr!("boss.plan_select"));
        };
        let session = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id);
        let archived = session.is_none_or(|session| session.archived_at.is_some());
        let (state_label, action) = if plan.finalized_at.is_some() {
            (
                tr!(
                    "boss.plan_finalized_ago",
                    ago = sidebar::format_time_ago(
                        unix_time().saturating_sub(plan.finalized_at.unwrap_or(0))
                    )
                ),
                session.map(|_| tr!("boss.plan_view_discussion")),
            )
        } else if archived {
            (tr!("boss.plan_archived"), None)
        } else {
            (
                tr!("boss.plan_in_discussion"),
                session.map(|_| tr!("boss.plan_open_discussion")),
            )
        };
        let header = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(10.0))
            .border_b_1()
            .border_color(theme.separator)
            .child(icon("icons/map.svg", 14.0, theme.text_secondary))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap(px(8.0))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(plan.idea.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(11.5))
                            .text_color(theme.text_tertiary)
                            .child(state_label),
                    ),
            )
            .when_some(action, |row, label| {
                row.child(
                    boss_button("boss-plan-discussion", label.clone(), &theme)
                        .child(icon("icons/message-square.svg", 13.0, theme.text_secondary))
                        .child(label)
                        .on_activation(cx, move |this, _, cx| {
                            this.request_session_activation(
                                session_id,
                                SessionActivationTransition::Visit,
                                cx,
                            )
                        }),
                )
            });
        self.ensure_plan_doc(key, session_id, &plan.plan_file, false, cx);
        let body = match self
            .plan_docs
            .get(&session_id)
            .and_then(|doc| doc.content.as_ref())
        {
            Some(Ok(text)) => {
                let text = text.clone();
                self.plan_document_view(session_id, &text, false, None, cx)
                    .into_any_element()
            }
            Some(Err(error)) => empty(format!("{}\n{error}", tr!("boss.plan_unavailable"))),
            None => empty(tr!("boss.loading")),
        };
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(header)
            .child(body)
            .into_any_element()
    }

    // ── Deliverables ─────────────────────────────────────────────────────

    /// The deliverable library — every published record, not just the
    /// sidebar's 12-hour recents, behind All/Pinned/Dormant/Archived.
    fn render_boss_deliverables_section(
        &mut self,
        key: DaemonKey,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let filter = self
            .boss_ui
            .deliverables_filter
            .get(&key)
            .copied()
            .unwrap_or_default();
        let segmented = self.boss_segmented(
            "boss-deliverables-filter",
            vec![
                (BossDeliverablesFilter::All, tr!("boss.deliverables_all")),
                (
                    BossDeliverablesFilter::Pinned,
                    tr!("boss.deliverables_pinned"),
                ),
                (
                    BossDeliverablesFilter::Dormant,
                    tr!("boss.deliverables_dormant"),
                ),
                (
                    BossDeliverablesFilter::Archived,
                    tr!("boss.deliverables_archived"),
                ),
            ],
            filter,
            cx,
            move |this, picked, _, cx| {
                this.boss_ui.deliverables_filter.insert(key, picked);
                this.sync_boss_page_rows();
                cx.notify();
            },
        );
        let body: AnyElement = if self.boss_ui.rows.is_empty() {
            let title = match filter {
                BossDeliverablesFilter::All => tr!("boss.deliverables_empty_all"),
                BossDeliverablesFilter::Pinned => tr!("boss.deliverables_empty_pinned"),
                BossDeliverablesFilter::Dormant => tr!("boss.deliverables_empty_dormant"),
                BossDeliverablesFilter::Archived => tr!("boss.deliverables_empty_archived"),
            };
            boss_empty_state(
                &theme,
                "icons/file-text.svg",
                title,
                tr!("boss.deliverables_empty_hint"),
            )
            .into_any_element()
        } else {
            self.boss_item_list(cx).into_any_element()
        };
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(theme.separator)
                    .child(segmented)
                    .child(div().flex_1()),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH + 48.0))
                    .mx_auto()
                    .px(px(16.0))
                    .py(px(6.0))
                    .flex()
                    .flex_col()
                    .child(body),
            )
            .into_any_element()
    }

    /// A library deliverable row — the sidebar row's name/type/age/unread
    /// cues at task-row density, opening the same preview page on click.
    /// Remote records keep their host in the detail line and offer no
    /// local Finder actions.
    fn render_boss_deliverable_row(
        &self,
        key: DaemonKey,
        deliverable_id: Uuid,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(deliverable) = self
            .boss_ui
            .states
            .get(&key)
            .and_then(|state| {
                state
                    .deliverables
                    .iter()
                    .find(|deliverable| deliverable.id == deliverable_id)
            })
            .cloned()
        else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let path = PathBuf::from(&deliverable.path);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&deliverable.path)
            .to_owned();
        let detail_icon = if deliverable.directory {
            "icons/folder.svg"
        } else {
            right_panel::file_icon_for_path(&deliverable.path)
        };
        let file_name = match key {
            DaemonKey::Remote(host) => match self.remote_host_name(host) {
                Some(host) => format!("{file_name} · {host}"),
                None => file_name,
            },
            DaemonKey::Local => file_name,
        };
        let age = sidebar::format_time_ago(unix_time().saturating_sub(deliverable.updated_at));
        let unread = deliverable
            .viewed_at
            .is_none_or(|viewed| viewed < deliverable.updated_at);
        let pinned = deliverable.pinned_at.is_some();
        let dormant = deliverable.dormant_at.is_some() && !pinned;
        let archived = deliverable.archived_at.is_some();
        let local = key == DaemonKey::Local;
        let menu = self.menu_handle(format!("boss-deliverable-{key:?}-{deliverable_id}"), cx);
        let keyboard_menu = menu.clone();
        let row_focus = menu.trigger_focus_handle().clone();
        let waku = cx.entity().downgrade();
        let row = div()
            .id(SharedString::from(format!(
                "boss-deliverable-row-{key:?}-{deliverable_id}"
            )))
            .track_focus(&row_focus)
            .tab_index(0)
            .h(px(42.0))
            .w_full()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .tooltip(Tooltip::text(
                deliverable
                    .source_path
                    .clone()
                    .unwrap_or_else(|| deliverable.path.clone()),
            ))
            .on_activation(cx, move |this, _, cx| {
                this.open_deliverable_task(key, deliverable_id, cx)
            })
            .on_key_down(cx.listener(move |_, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.as_str() == "f10" && event.keystroke.modifiers.shift {
                    keyboard_menu.open_context_menu(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(icon(detail_icon, 15.0, theme.text_secondary))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .min_w_0()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(sp(14.0))
                                    .line_height(sp(17.0))
                                    .text_color(theme.text)
                                    .child(deliverable.name.clone()),
                            )
                            .when(unread, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .size(px(7.0))
                                        .rounded_full()
                                        .bg(theme.info),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .min_w_0()
                            .text_size(sp(12.5))
                            .line_height(sp(15.0))
                            .text_color(theme.text_tertiary)
                            .child(div().min_w_0().truncate().child(file_name))
                            .when(dormant, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .text_color(theme.text_ghost)
                                        .child(tr!("boss.deliverable_dormant")),
                                )
                            })
                            .when(archived, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .text_color(theme.text_ghost)
                                        .child(tr!("boss.deliverable_archived_tag")),
                                )
                            }),
                    ),
            )
            .when(pinned, |row| {
                row.child(icon("icons/pin-filled.svg", 12.0, theme.text_tertiary))
            })
            .child(
                div()
                    .flex_none()
                    .w(px(72.0))
                    .text_right()
                    .text_size(sp(12.0))
                    .text_color(theme.text_ghost)
                    .child(age),
            );
        context_menu(
            div().w_full().child(row),
            SharedString::from(format!("boss-deliverable-menu-{key:?}-{deliverable_id}")),
            &menu,
            move |_cx| {
                let mut items = Vec::new();
                if local {
                    items.push(
                        MenuItem::new(tr!("deliverable.open"), {
                            let path = path.clone();
                            move |_, cx| crate::platform::open_with_default_app(&path, cx)
                        })
                        .icon("icons/external-link.svg"),
                    );
                    items.push(
                        MenuItem::new(tr!("common.reveal_in_finder"), {
                            let path = path.clone();
                            move |_, cx| crate::platform::reveal_in_file_manager(&path, cx)
                        })
                        .icon("icons/folder-open.svg"),
                    );
                    items.push(MenuItem::Separator);
                }
                {
                    let waku = waku.clone();
                    items.push(
                        MenuItem::new(
                            if pinned {
                                tr!("session.unpin")
                            } else {
                                tr!("session.pin")
                            },
                            move |_, cx| {
                                let _ = waku.update(cx, |waku, cx| {
                                    waku.set_deliverable_pinned(key, deliverable_id, !pinned, cx);
                                });
                            },
                        )
                        .icon(if pinned {
                            "icons/pin-off.svg"
                        } else {
                            "icons/pin.svg"
                        }),
                    );
                }
                {
                    let waku = waku.clone();
                    items.push(
                        MenuItem::new(
                            if dormant {
                                tr!("session.restore")
                            } else {
                                tr!("session.sweep")
                            },
                            move |_, cx| {
                                let _ = waku.update(cx, |waku, cx| {
                                    waku.set_deliverable_dormant(key, deliverable_id, !dormant, cx);
                                });
                            },
                        )
                        .icon(if dormant {
                            "icons/rotate-cw.svg"
                        } else {
                            "icons/broom.svg"
                        }),
                    );
                }
                {
                    let waku = waku.clone();
                    items.push(
                        MenuItem::new(
                            if archived {
                                tr!("common.unarchive")
                            } else {
                                tr!("session.archive")
                            },
                            move |_, cx| {
                                let _ = waku.update(cx, |waku, cx| {
                                    waku.set_deliverable_archived(
                                        key,
                                        deliverable_id,
                                        !archived,
                                        cx,
                                    );
                                });
                            },
                        )
                        .icon("icons/archive.svg"),
                    );
                }
                items.push(MenuItem::Separator);
                let dismiss_waku = waku.clone();
                items.push(
                    MenuItem::new(tr!("deliverable.dismiss"), move |_, cx| {
                        let _ = dismiss_waku.update(cx, |waku, cx| {
                            waku.boss_request(
                                key,
                                BossOperation::DismissDeliverable { id: deliverable_id },
                                BossReply::List,
                                cx,
                            );
                        });
                    })
                    .icon("icons/trash.svg"),
                );
                items
            },
        )
        .into_any_element()
    }

    fn render_boss_item(&self, item: BossItem, cx: &mut Context<Self>) -> AnyElement {
        let Some((key, _)) = self.boss_ui.page else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        match item {
            BossItem::Employee(id) => self.render_boss_employee_page_row(id, key, cx),
            BossItem::Deliverable(id) => self.render_boss_deliverable_row(key, id, cx),
            BossItem::HistoryHit(index) => self.render_boss_history_hit(key, index, cx),
            BossItem::HistoryFooter => self.render_boss_history_footer(key, cx),
            BossItem::MemoryRecord(index) => self.render_boss_memory_record(key, index, cx),
            BossItem::MemoryFooter { remaining } => {
                // Reveal the next filtered page in place — a tail splice, so
                // the rows above it keep their scroll position.
                div()
                    .id("boss-memory-footer")
                    .px(px(16.0))
                    .py(px(10.0))
                    .flex()
                    .justify_center()
                    .child(
                        boss_button(
                            "boss-memory-show-more",
                            tr!("boss.memory_show_more", count = remaining),
                            &theme,
                        )
                        .child(tr!("boss.memory_show_more", count = remaining))
                        .on_activation(cx, move |this, _, cx| {
                            if let Some(feed) = this.boss_ui.memory_feed.get_mut(&key) {
                                feed.visible = feed.visible.saturating_add(MEMORY_FEED_PAGE);
                            }
                            this.sync_boss_page_rows();
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            }
            BossItem::Plan(session_id) => {
                let Some(plan) = self
                    .boss_ui
                    .states
                    .get(&key)
                    .and_then(|state| {
                        state
                            .planning
                            .iter()
                            .find(|plan| plan.session_id == session_id)
                    })
                    .cloned()
                else {
                    return div().into_any_element();
                };
                let selected = self.boss_ui.plans_selected.get(&key) == Some(&session_id);
                let session = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id);
                let archived = session.is_none_or(|session| session.archived_at.is_some());
                let detail = if let Some(finalized_at) = plan.finalized_at {
                    tr!(
                        "boss.plan_finalized_ago",
                        ago = sidebar::format_time_ago(unix_time().saturating_sub(finalized_at))
                    )
                } else if archived {
                    tr!("boss.plan_archived")
                } else {
                    tr!("boss.plan_in_discussion")
                };
                div()
                    .id(SharedString::from(format!("boss-plan-{session_id}")))
                    .tab_index(0)
                    .h(px(42.0))
                    .w_full()
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .cursor_pointer()
                    .when(selected, |row| row.bg(theme.overlay))
                    .hover(|style| style.bg(theme.overlay))
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .on_activation(cx, move |this, _, cx| {
                        this.boss_ui.plans_selected.insert(key, session_id);
                        cx.notify();
                    })
                    .child(icon("icons/compass.svg", 15.0, theme.text_secondary))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_color(theme.text)
                                    .child(plan.idea.clone()),
                            )
                            .child(
                                div()
                                    .text_size(sp(11.0))
                                    .text_color(theme.text_tertiary)
                                    .truncate()
                                    .child(detail),
                            ),
                    )
                    .into_any_element()
            }
            BossItem::Persona(id) => {
                let Some(state) = self.boss_ui.states.get(&key).cloned() else {
                    return div().into_any_element();
                };
                let Some(persona) = state
                    .personas
                    .iter()
                    .find(|persona| persona.id == id)
                    .cloned()
                else {
                    return div().into_any_element();
                };
                self.render_boss_persona_row(
                    key,
                    &persona,
                    false,
                    self.boss_ui.personas_selected.get(&key).copied(),
                    &theme,
                    cx,
                )
            }
        }
    }

    /// One Memory Records row — bucket label and muted relative creation
    /// time on one line (the exact timestamp rides the tooltip), the note's
    /// text beneath, and the correction affordance at the row's right edge.
    /// Read-only; corrections still arm Boss chat rather than editing.
    fn render_boss_memory_record(
        &self,
        key: DaemonKey,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(feed) = self.boss_ui.memory_feed.get(&key) else {
            return div().into_any_element();
        };
        let Some(record) = feed.records.get(index).cloned() else {
            return div().into_any_element();
        };
        let record_key = record.key();
        let expanded = feed.expanded.contains(&record_key);
        let group = SharedString::from(format!("boss-memory-row-{record_key}"));
        let age = memory_record_age(unix_time().saturating_sub(record.created_at));
        let exact = memory_record_exact_time(record.created_at);
        let mut provenance = format!("{} · {}", record.bucket, age);
        if let Some(project_name) = record
            .project
            .as_ref()
            .and_then(|project| project.name.as_deref())
            .filter(|name| !name.eq_ignore_ascii_case(&record.bucket))
        {
            provenance.push_str(" · ");
            provenance.push_str(project_name);
        }
        let correct_label = format!("buckets/{}/note-{}", record.bucket, record.sequence);
        let correct_content = record.text.clone();
        let correct_name = format!(
            "{} — {}",
            tr!("boss.ask_correct"),
            record.text.chars().take(80).collect::<String>()
        );
        let long_text = record.text.len() > MEMORY_EXPAND_CHARS
            || record.text.lines().nth(MEMORY_EXPAND_LINES).is_some();
        let mut row = div()
            .id(SharedString::from(format!("boss-memory-{record_key}")))
            .tab_index(0)
            .group(group.clone())
            .w_full()
            .px(px(16.0))
            .py(px(12.0))
            .border_b_1()
            .border_color(theme.separator)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .aria_label(tr!(
                "boss.memory_record_label",
                bucket = record.bucket.clone(),
                when = exact.clone()
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "boss-memory-time-{record_key}"
                            )))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(11.0))
                            .text_color(theme.text_tertiary)
                            .tooltip(Tooltip::text(exact.clone()))
                            .child(provenance),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "boss-memory-correct-{record_key}"
                            )))
                            .tab_index(0)
                            .flex_none()
                            .p(px(4.0))
                            .rounded(px(4.0))
                            .cursor_pointer()
                            .opacity(0.45)
                            .group_hover(group.clone(), |style| style.opacity(1.0))
                            .hover(|style| style.bg(theme.overlay))
                            .focus_visible(|style| {
                                style.bg(theme.focus_highlight()).opacity(1.0)
                            })
                            .tooltip(Tooltip::text(tr!("boss.ask_correct")))
                            .aria_label(correct_name)
                            .child(icon("icons/message-square.svg", 13.0, theme.text_secondary))
                            .on_activation(cx, move |this, _, cx| {
                                this.chat_with_boss(key, cx);
                                this.boss_ui.command_memory_correction = Some((
                                    key,
                                    correct_label.clone(),
                                    correct_content.clone(),
                                ));
                                this.sync_composer_placeholder(cx);
                            }),
                    ),
            )
            .child(
                div()
                    .pt(px(4.0))
                    .text_size(sp(13.0))
                    .line_height(sp(19.0))
                    .when(!expanded, |element| {
                        element.line_clamp(MEMORY_EXPAND_LINES)
                    })
                    .child(record.text.clone()),
            );
        if long_text {
            let expanding = !expanded;
            row = row.child(
                div()
                    .id(SharedString::from(format!(
                        "boss-memory-expand-{record_key}"
                    )))
                    .tab_index(0)
                    .pt(px(4.0))
                    .text_size(sp(11.5))
                    .text_color(theme.text_secondary)
                    .cursor_pointer()
                    .hover(|style| style.text_color(theme.text))
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .aria_label(if expanded {
                        tr!("boss.memory_collapse")
                    } else {
                        tr!("boss.memory_expand")
                    })
                    .child(if expanded {
                        tr!("boss.memory_collapse")
                    } else {
                        tr!("boss.memory_expand")
                    })
                    .on_activation(cx, move |this, _, cx| {
                        if let Some(feed) = this.boss_ui.memory_feed.get_mut(&key) {
                            if expanding {
                                feed.expanded.insert(record_key.clone());
                            } else {
                                feed.expanded.remove(&record_key);
                            }
                        }
                        if let Some(row_index) = this
                            .boss_ui
                            .rows
                            .iter()
                            .position(|item| *item == BossItem::MemoryRecord(index))
                        {
                            this.boss_ui.list.remeasure_items(row_index..row_index + 1);
                        }
                        cx.notify();
                    }),
            );
        }
        row.into_any_element()
    }

    /// The persona form behind the detail pane's Edit action. Saving names
    /// its scope beside the controls — the defaults reach employees
    /// summoned after the save; existing grants change through the boss.
    fn render_boss_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(editor) = &self.boss_ui.editor else {
            return boss_detail_placeholder(
                &theme,
                "icons/user-round.svg",
                tr!("boss.persona_select"),
            );
        };
        if self.boss_ui.page != Some((editor.key, BossTab::Personas)) {
            return boss_detail_placeholder(
                &theme,
                "icons/user-round.svg",
                tr!("boss.persona_select"),
            );
        }
        let mut form = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(tr!("boss.name_path"))
            .child(boss_input(editor.name.clone(), &theme))
            .child(tr!("boss.markdown"))
            .child(boss_input(editor.content.clone(), &theme))
            .child(tr!("boss.persona_buckets"))
            .child(boss_input(editor.buckets.clone(), &theme))
            .child(tr!("boss.pinned_files"))
            .child(boss_input(editor.pinned.clone(), &theme))
            .child(tr!("boss.persona_icon_hint"))
            .child(
                div().flex().flex_wrap().gap(px(4.0)).children(
                    std::iter::once(None)
                        .chain(CustomCommandIcon::EMPLOYEE.into_iter().map(Some))
                        .map(|choice| {
                            let selected = editor.icon == choice;
                            let label = choice.map_or("None", CustomCommandIcon::label);
                            let icon_path = choice.map(crate::custom_commands::icon_path);
                            boss_button(format!("persona-icon-{label}"), label, &theme)
                                .child(if let Some(path) = icon_path {
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .child(icon(path, 14.0, theme.text_secondary))
                                        .child(label)
                                } else {
                                    div().child(label)
                                })
                                .when(selected, |button| button.bg(theme.overlay_strong))
                                .on_activation(cx, move |this, _, cx| {
                                    if let Some(editor) = &mut this.boss_ui.editor {
                                        editor.icon = choice;
                                    }
                                    cx.notify();
                                })
                        }),
                ),
            )
            .child(tr!("boss.permissions_hint"));
        for (label, enabled) in [
            ("boss.delegate", editor.permissions.summon_employees),
            ("boss.computer", editor.permissions.computer_use),
        ] {
            form = form.child(
                boss_button(label, tr!(label), &theme)
                    .child(icon(
                        if enabled {
                            "icons/check.svg"
                        } else {
                            "icons/x.svg"
                        },
                        13.0,
                        theme.text_secondary,
                    ))
                    .child(tr!(label))
                    .on_activation(cx, move |this, _, cx| {
                        if let Some(editor) = &mut this.boss_ui.editor {
                            match label {
                                "boss.delegate" => {
                                    editor.permissions.summon_employees =
                                        !editor.permissions.summon_employees;
                                }
                                _ => {
                                    editor.permissions.computer_use =
                                        !editor.permissions.computer_use;
                                }
                            }
                        }
                        cx.notify();
                    }),
            );
        }
        for integration in &editor.integrations {
            let id = integration.clone();
            let enabled = editor.permissions.integration_ids.contains(&id);
            form = form.child(
                boss_button(format!("boss-integration-{id}"), id.clone(), &theme)
                    .child(icon(
                        if enabled {
                            "icons/check.svg"
                        } else {
                            "icons/x.svg"
                        },
                        13.0,
                        theme.text_secondary,
                    ))
                    .child(id.clone())
                    .on_activation(cx, move |this, _, cx| {
                        if let Some(editor) = &mut this.boss_ui.editor {
                            if editor.permissions.integration_ids.contains(&id) {
                                editor
                                    .permissions
                                    .integration_ids
                                    .retain(|value| value != &id);
                            } else {
                                editor.permissions.integration_ids.push(id.clone());
                            }
                        }
                        cx.notify();
                    }),
            );
        }
        let editing_base = self
            .boss_ui
            .states
            .get(&editor.key)
            .is_some_and(|state| state.employee_persona_id == Some(editor.persona));
        let body = div()
            .flex_1()
            .min_h_0()
            .id("boss-editor-scroll")
            .overflow_y_scroll()
            .p(px(20.0))
            .child(
                form.child(
                    div()
                        .text_size(sp(12.0))
                        .text_color(theme.text_tertiary)
                        .child(if editing_base {
                            tr!("boss.persona_scope_base_hint")
                        } else {
                            tr!("boss.persona_scope_hint")
                        }),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            boss_button("boss-save", tr!("boss.save"), &theme)
                                .child(icon("icons/check.svg", 14.0, theme.text_secondary))
                                .child(tr!("boss.save"))
                                .on_activation(cx, |this, _, cx| this.save_boss_document(cx)),
                        )
                        .child(
                            boss_button("boss-discard", tr!("boss.discard"), &theme)
                                .child(icon("icons/x.svg", 14.0, theme.text_secondary))
                                .child(tr!("boss.discard"))
                                .on_activation(cx, |this, _, cx| {
                                    this.boss_ui.editor = None;
                                    cx.notify();
                                }),
                        ),
                ),
            );
        body.into_any_element()
    }
}

/// One label/value line of a detail pane's info table — the Skills
/// detail's pattern.
fn boss_info_row(theme: &Theme, label: String, value: AnyElement, last: bool) -> Div {
    div()
        .py(px(8.0))
        .when(!last, |element| {
            element.border_b_1().border_color(theme.separator)
        })
        .flex()
        .items_baseline()
        .gap(px(12.0))
        .child(
            div()
                .w(px(110.0))
                .flex_none()
                .text_size(sp(12.5))
                .text_color(theme.text_tertiary)
                .child(SharedString::from(label)),
        )
        .child(div().flex_1().min_w_0().flex().child(value))
}

fn boss_plain_value(theme: &Theme, value: String) -> AnyElement {
    div()
        .text_size(sp(12.5))
        .text_color(theme.text_secondary)
        .child(SharedString::from(value))
        .into_any_element()
}

/// A section list's small group label — the Skills list's section
/// headers without the count.
fn boss_section_label(theme: &Theme, label: String, first: bool) -> Div {
    div()
        .w_full()
        .pt(px(if first { 6.0 } else { 14.0 }))
        .pb(px(4.0))
        .px(px(10.0))
        .flex()
        .items_baseline()
        .child(
            div()
                .text_size(sp(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text_tertiary)
                .child(SharedString::from(label.to_uppercase())),
        )
}

/// The quiet whole-section empty state — centered icon tile, title, and a
/// line of explanation, the Skills page's version.
fn boss_empty_state(theme: &Theme, icon_path: &'static str, title: String, hint: String) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .px(px(40.0))
        .py(px(40.0))
        .child(
            div()
                .w(px(44.0))
                .h(px(44.0))
                .rounded(px(13.0))
                .bg(theme.overlay)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(icon_path, 21.0, theme.text_tertiary)),
        )
        .child(
            div()
                .text_size(sp(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .text_center()
                .child(title),
        )
        .child(
            div()
                .max_w(px(420.0))
                .text_size(sp(12.5))
                .line_height(sp(17.0))
                .text_color(theme.text_secondary)
                .text_center()
                .child(hint),
        )
}

/// The quiet right-pane placeholder for a list/detail section with no
/// selection — centered glyph and one line.
fn boss_detail_placeholder(theme: &Theme, icon_path: &'static str, label: String) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(icon(icon_path, 22.0, theme.text_ghost))
        .child(
            div()
                .text_size(sp(12.5))
                .text_color(theme.text_ghost)
                .child(label),
        )
        .into_any_element()
}

fn boss_loading_label_visible(pending: bool) -> bool {
    pending
}

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 1-based admission order among the daemon's queued employees — errands
/// included, so a goal's neutral wait detail accounts for the hidden work
/// ahead of it without revealing a position number.
fn boss_queue_ranks(state: &BossState) -> HashMap<Uuid, usize> {
    let mut queued: Vec<&waku_protocol::boss::BossEmployee> = state
        .employees
        .iter()
        .filter(|employee| employee.lifecycle() == waku_protocol::boss::EmployeeLifecycle::Queued)
        .collect();
    queued.sort_by_key(|employee| {
        employee
            .ticket
            .as_ref()
            .map(|ticket| ticket.sequence)
            .unwrap_or(u64::MAX)
    });
    queued
        .iter()
        .enumerate()
        .map(|(index, employee)| (employee.session_id, index + 1))
        .collect()
}

/// Whether an employee record offers the manual Resume action: the
/// daemon's `resume` op re-admits an expired record in place with its
/// transcript, workspace, and provider cursor intact. A live record
/// takes a prompt instead, so the action stays hidden until expiry; an
/// expiry flagged unresumable renders disabled rather than absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EmployeeResume {
    Hidden,
    Enabled,
    Disabled,
}

fn employee_resume_action(employee: &waku_protocol::boss::BossEmployee) -> EmployeeResume {
    if !employee.expired {
        return EmployeeResume::Hidden;
    }
    if employee
        .expiry
        .as_ref()
        .is_some_and(|expiry| !expiry.resumable)
    {
        EmployeeResume::Disabled
    } else {
        EmployeeResume::Enabled
    }
}

/// A queued employee's wait reason for the sidebar row tooltip and the
/// Goals Pending row — the ticket's admission blocker when the scheduler
/// recorded one, else a neutral admission label. `rank` is the employee's
/// [`boss_queue_ranks`] position; non-queued employees return `None`.
fn boss_queue_detail(
    employee: &waku_protocol::boss::BossEmployee,
    rank: Option<usize>,
) -> Option<String> {
    if employee.lifecycle() != waku_protocol::boss::EmployeeLifecycle::Queued {
        return None;
    }
    let detail = match employee
        .ticket
        .as_ref()
        .and_then(|ticket| ticket.blocked_by.first())
    {
        Some(waku_protocol::boss::AdmissionBlocker::ModelLimit { used, limit }) => tr!(
            "boss.goals_queue_model",
            model = employee
                .ticket
                .as_ref()
                .map(|ticket| ticket.model.as_str())
                .unwrap_or_default(),
            used = used,
            limit = limit
        ),
        Some(waku_protocol::boss::AdmissionBlocker::HostResources { detail })
        | Some(waku_protocol::boss::AdmissionBlocker::OutcomeWait { detail })
        | Some(waku_protocol::boss::AdmissionBlocker::EmployeeBase { detail }) => detail.clone(),
        None => {
            if rank == Some(1) {
                tr!("boss.goals_queue_admission")
            } else {
                tr!("boss.goals_queue_earlier")
            }
        }
    };
    Some(detail)
}

#[track_caller]
fn boss_sidebar_label(
    name: String,
    job_title: String,
    job_icon: Option<&'static str>,
    show_job_icon: bool,
    theme: &Theme,
    provider: Option<ProviderKind>,
) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(sp(14.0))
                .line_height(sp(17.0))
                .text_color(theme.text)
                .truncate()
                .child(name),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .text_size(sp(13.0))
                .line_height(sp(15.0))
                .text_color(theme.text_tertiary)
                .when_some(provider, |row, provider| {
                    row.child(crate::ui::provider_mark(
                        provider,
                        12.0,
                        theme.text_tertiary,
                    ))
                })
                .when(
                    show_job_icon && !job_title.is_empty() && provider.is_none(),
                    |row| {
                        row.child(icon(
                            job_icon.unwrap_or_else(|| job_title_icon(&job_title)),
                            12.0,
                            theme.text_tertiary,
                        ))
                    },
                )
                .child(div().min_w_0().truncate().child(job_title)),
        )
}

/// Fallback icon for job titles that match no category — an agent without
/// a robot motif.
const JOB_TITLE_FALLBACK_ICON: &str = "icons/brain.svg";

/// Last-resort employee icon categories, in precedence order: the first
/// category whose keyword list contains a complete title word wins, so
/// specialized work (bug fixing, localization, packaging, infrastructure,
/// database) is listed ahead of generic implementation. Keywords match
/// exact words only — no stemming or prefix matching.
const JOB_TITLE_ICON_CATEGORIES: &[(&[&str], &str)] = &[
    (&["bug", "fix", "bugfix", "debug"], "icons/bug.svg"),
    (
        &[
            "translate",
            "translation",
            "localize",
            "localization",
            "l10n",
            "i18n",
        ],
        "icons/languages.svg",
    ),
    (
        &[
            "dependency",
            "dependencies",
            "package",
            "packaging",
            "publish",
            "publishing",
        ],
        "icons/package.svg",
    ),
    (
        &[
            "infrastructure",
            "infra",
            "deploy",
            "deployment",
            "daemon",
            "server",
        ],
        "icons/server.svg",
    ),
    (
        &[
            "database",
            "databases",
            "sql",
            "query",
            "queries",
            "schema",
            "schemas",
        ],
        "icons/database.svg",
    ),
    (
        &["implement", "build", "migrate", "refactor"],
        "icons/terminal.svg",
    ),
    (&["investigate"], "icons/search.svg"),
    (&["verify", "validate", "smoke"], "icons/circle-check.svg"),
    (&["test", "qa"], "icons/beaker.svg"),
    (
        &["integrate", "merge", "land", "release"],
        "icons/git-merge.svg",
    ),
    (&["review", "audit"], "icons/eye.svg"),
    (&["design", "spec"], "icons/pencil.svg"),
    (&["research", "analyze"], "icons/folder-search.svg"),
    (&["write", "docs"], "icons/file-text.svg"),
    (&["security", "privacy"], "icons/lock.svg"),
    (&["measure", "benchmark", "perf"], "icons/gauge.svg"),
    (&["port", "migrate"], "icons/fork.svg"),
];

/// The icon a job title earns when no explicit icon was chosen: the first
/// category containing one of the title's complete words, or the brain
/// fallback. A word is a maximal run of Unicode letters or digits —
/// spaces, punctuation, hyphens, and underscores separate words — matched
/// case-insensitively against the category keywords.
pub(super) fn job_title_icon(title: &str) -> &'static str {
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    JOB_TITLE_ICON_CATEGORIES
        .iter()
        .find(|(keywords, _)| {
            keywords
                .iter()
                .any(|keyword| words.iter().any(|word| word == keyword))
        })
        .map(|(_, icon)| *icon)
        .unwrap_or(JOB_TITLE_FALLBACK_ICON)
}

/// Every icon path the classifier can emit — the category table plus the
/// fallback — for the asset guard in `crate::assets`.
#[cfg(test)]
pub(crate) fn job_title_icon_paths() -> impl Iterator<Item = &'static str> {
    JOB_TITLE_ICON_CATEGORIES
        .iter()
        .map(|(_, icon)| *icon)
        .chain([JOB_TITLE_FALLBACK_ICON])
}

#[cfg(test)]
mod job_title_icon_tests {
    use super::{JOB_TITLE_FALLBACK_ICON, JOB_TITLE_ICON_CATEGORIES, job_title_icon};

    #[test]
    fn every_keyword_resolves_to_its_category() {
        let cases: &[(&str, &str)] = &[
            // Bug fixing precedes every other category.
            ("bug sweep", "icons/bug.svg"),
            ("hot fix", "icons/bug.svg"),
            ("bugfix release", "icons/bug.svg"),
            ("Debug Build Failure", "icons/bug.svg"),
            // Specialized categories beat generic implementation.
            ("Translate Strings", "icons/languages.svg"),
            ("Translation pass", "icons/languages.svg"),
            ("Localize the app", "icons/languages.svg"),
            ("Localization Crate Extraction", "icons/languages.svg"),
            ("l10n audit", "icons/languages.svg"),
            ("i18n support", "icons/languages.svg"),
            ("Dependency Build", "icons/package.svg"),
            ("untangle dependencies", "icons/package.svg"),
            ("package the release", "icons/package.svg"),
            ("packaging cleanup", "icons/package.svg"),
            ("Publish Docs Site", "icons/package.svg"),
            ("publishing pipeline", "icons/package.svg"),
            ("infrastructure refresh", "icons/server.svg"),
            ("infra work", "icons/server.svg"),
            ("Deploy Preview", "icons/server.svg"),
            ("deployment checklist", "icons/server.svg"),
            ("Daemon Refactor", "icons/server.svg"),
            ("server migration", "icons/server.svg"),
            ("Database work", "icons/database.svg"),
            ("sync databases", "icons/database.svg"),
            ("SQL Tuning", "icons/database.svg"),
            ("slow query", "icons/database.svg"),
            ("answer queries", "icons/database.svg"),
            ("Schema Migration", "icons/database.svg"),
            ("split schemas", "icons/database.svg"),
            ("Implement Feature", "icons/terminal.svg"),
            ("Build System", "icons/terminal.svg"),
            ("Migrate Store", "icons/terminal.svg"),
            ("Refactor Parser", "icons/terminal.svg"),
            ("Investigate Failure", "icons/search.svg"),
            ("Verify Steps", "icons/circle-check.svg"),
            ("Validate Results", "icons/circle-check.svg"),
            ("smoke check", "icons/circle-check.svg"),
            ("Test Runner", "icons/beaker.svg"),
            ("QA Pass", "icons/beaker.svg"),
            ("Integrate Branch", "icons/git-merge.svg"),
            ("Merge Queue", "icons/git-merge.svg"),
            ("Land Branch", "icons/git-merge.svg"),
            ("Release Train", "icons/git-merge.svg"),
            ("Review Diff", "icons/eye.svg"),
            ("Audit Logs", "icons/eye.svg"),
            ("Design Mockup", "icons/pencil.svg"),
            ("Spec Draft", "icons/pencil.svg"),
            ("Research Options", "icons/folder-search.svg"),
            ("Analyze Traces", "icons/folder-search.svg"),
            ("Write Docs", "icons/file-text.svg"),
            ("docs refresh", "icons/file-text.svg"),
            ("Security Hardening", "icons/lock.svg"),
            ("Privacy Pass", "icons/lock.svg"),
            ("Measure Startup", "icons/gauge.svg"),
            ("Benchmark Suite", "icons/gauge.svg"),
            ("perf work", "icons/gauge.svg"),
            ("Port Driver", "icons/fork.svg"),
        ];
        for (title, expected) in cases {
            assert_eq!(job_title_icon(title), *expected, "{title}");
        }
    }

    #[test]
    fn matches_only_complete_words() {
        let brain = JOB_TITLE_FALLBACK_ICON;
        for title in [
            "Local Cache Extraction", // "Local" is not "localize".
            "building",               // no stemming.
            "support",                // not "port".
            "performance",            // not "perf".
            "prefix",
            "Data Migration", // broad words stay unmatched.
            "migrate1234",    // a word runs through digits.
        ] {
            assert_eq!(job_title_icon(title), brain, "{title}");
        }
        // Punctuation, hyphens, and underscores separate words.
        assert_eq!(job_title_icon("smoke-test"), "icons/circle-check.svg");
        assert_eq!(job_title_icon("code_review"), "icons/eye.svg");
        assert_eq!(job_title_icon("qa-run"), "icons/beaker.svg");
        assert_eq!(job_title_icon("fix: regression"), "icons/bug.svg");
        assert_eq!(job_title_icon("write (docs)"), "icons/file-text.svg");
        assert_eq!(job_title_icon(""), brain);
        assert_eq!(job_title_icon("   "), brain);
        assert_eq!(job_title_icon("Uncategorized role"), brain);
    }

    #[test]
    fn category_order_breaks_ties() {
        // "Localization" (2) beats "Extraction"; specialized categories beat
        // implementation (6) regardless of title word order.
        assert_eq!(job_title_icon("Debug Build Failure"), "icons/bug.svg");
        assert_eq!(job_title_icon("Dependency Build"), "icons/package.svg");
        assert_eq!(job_title_icon("Localization Build"), "icons/languages.svg");
        assert_eq!(job_title_icon("Daemon Refactor"), "icons/server.svg");
        assert_eq!(job_title_icon("Schema Migration"), "icons/database.svg");
        // "migrate" belongs to implementation (6) and porting (17);
        // implementation wins, so only "port" reaches fork.
        assert_eq!(job_title_icon("Migrate Database"), "icons/database.svg");
        assert_eq!(job_title_icon("Port and Migrate"), "icons/terminal.svg");
    }

    #[test]
    fn every_category_icon_is_bundled() {
        for (_, icon) in JOB_TITLE_ICON_CATEGORIES {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(icon);
            assert!(path.is_file(), "missing bundled icon {icon}");
        }
        let fallback = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(JOB_TITLE_FALLBACK_ICON);
        assert!(fallback.is_file(), "missing {JOB_TITLE_FALLBACK_ICON}");
    }
}

#[cfg(test)]
mod loading_indicator_tests {
    use super::boss_loading_label_visible;

    #[test]
    fn pending_request_shows_loading_label() {
        assert!(boss_loading_label_visible(true));
        assert!(!boss_loading_label_visible(false));
    }
}

#[track_caller]
fn boss_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id.into())
        .tab_index(0)
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(8.0))
        .border(hairline())
        .border_color(theme.border)
        .bg(theme.inset)
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(sp(12.0))
        .text_color(theme.text)
        .cursor_pointer()
        .hover(|style| style.bg(theme.overlay))
        .focus_visible(|style| style.bg(theme.focus_highlight()))
}

#[track_caller]
fn boss_input(input: Entity<TextInput>, theme: &Theme) -> Div {
    div()
        .w_full()
        .rounded(px(8.0))
        .border(hairline())
        .border_color(theme.border)
        .bg(theme.inset)
        .p(px(8.0))
        .child(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_client::boss::{BossDeliverable, BossResourcePolicy};

    fn deliverable_state(updated_at: u64, viewed_at: Option<u64>) -> BossState {
        BossState {
            identity: BossIdentity {
                id: Uuid::new_v4(),
                name: "Boss".into(),
                avatar_seed: String::new(),
                avatar_style: Default::default(),
            },
            persona_id: Uuid::new_v4(),
            employee_persona_id: None,
            specialist_persona_ids: Default::default(),
            persona_defaults: Default::default(),
            persona_default_notice: None,
            session_id: None,
            personas: Vec::new(),
            employees: Vec::new(),
            retired_employees: Vec::new(),
            deliverables: vec![BossDeliverable {
                id: Uuid::nil(),
                name: "report.md".into(),
                path: "/tmp/report.md".into(),
                source_path: None,
                directory: false,
                created_at: updated_at,
                updated_at,
                pinned_at: None,
                dormant_at: None,
                archived_at: None,
                viewed_at,
                plan_id: None,
            }],
            goals_viewed_at: None,
            planning: Vec::new(),
            outcomes: Vec::new(),
            resource_policy: BossResourcePolicy::default(),
            next_sequence: 0,
            next_event_id: 0,
            name_cursor: 0,
            outbox: Vec::new(),
            waves: Vec::new(),
            wave_outbox: Vec::new(),
            revision: 0,
        }
    }

    struct DeliverableHoverLayoutHarness;

    impl Render for DeliverableHoverLayoutHarness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let group = SharedString::from("deliverable-hover-layout");
            div().size_full().child(
                div()
                    .id("deliverable-layout-row")
                    .debug_selector(|| "deliverable-layout-row".into())
                    .group(group.clone())
                    .relative()
                    .w(px(240.0))
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(div().flex_1().min_w_0())
                    .child(
                        sidebar_deliverable_action_slot(group.clone())
                            .id("deliverable-layout-pin")
                            .debug_selector(|| "deliverable-layout-pin".into()),
                    )
                    .child(
                        sidebar_deliverable_action_slot(group.clone())
                            .id("deliverable-layout-archive")
                            .debug_selector(|| "deliverable-layout-archive".into()),
                    )
                    .child(sidebar_deliverable_unread_status_slot(
                        group,
                        SharedString::from("deliverable-layout-status"),
                        div().size(px(7.0)).rounded_full(),
                    )
                    .debug_selector(|| "deliverable-layout-status".into())),
            )
        }
    }

    #[gpui::test]
    fn deliverable_hover_actions_take_the_unread_status_slot(cx: &mut gpui::TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| DeliverableHoverLayoutHarness);
        cx.run_until_parked();

        let row = cx.debug_bounds("deliverable-layout-row").unwrap();
        let status = cx.debug_bounds("deliverable-layout-status").unwrap();
        let pin = cx.debug_bounds("deliverable-layout-pin").unwrap();
        let archive = cx.debug_bounds("deliverable-layout-archive").unwrap();
        assert_eq!(status.right(), row.right());
        assert_eq!(status.size.width, px(12.0));
        assert_eq!(pin.size.width, px(0.0));
        assert_eq!(archive.size.width, px(0.0));

        cx.simulate_mouse_move(row.center(), None, Modifiers::none());
        cx.run_until_parked();

        let status = cx.debug_bounds("deliverable-layout-status").unwrap();
        let pin = cx.debug_bounds("deliverable-layout-pin").unwrap();
        let archive = cx.debug_bounds("deliverable-layout-archive").unwrap();
        assert_eq!(status.right(), row.right());
        assert_eq!(status.size.width, px(12.0));
        assert_eq!(pin.size.width, px(20.0));
        assert_eq!(archive.size.width, px(20.0));
        assert_eq!(archive.right(), row.right());
        assert!(archive.origin.x < status.origin.x);
    }

    #[test]
    fn unread_deliverable_forgets_its_page_scroll_position() {
        let key = DaemonKey::Local;
        let page = (key, Uuid::nil());
        let mut ui = BossUi::default();
        let scrolled = ListState::new(8, ListAlignment::Top, px(1024.0));
        scrolled.scroll_to(gpui::ListOffset {
            item_ix: 5,
            offset_in_item: px(12.0),
        });
        ui.deliverable_page_scroll.insert(page, scrolled.clone());

        // Re-published after the last view: opening starts at the top.
        ui.states.insert(key, deliverable_state(200, Some(100)));
        ui.forget_unread_deliverable_scroll(key, Uuid::nil());
        assert!(!ui.deliverable_page_scroll.contains_key(&page));

        // Never opened at all reads the same way.
        ui.deliverable_page_scroll.insert(page, scrolled.clone());
        ui.states.insert(key, deliverable_state(200, None));
        ui.forget_unread_deliverable_scroll(key, Uuid::nil());
        assert!(!ui.deliverable_page_scroll.contains_key(&page));

        // Still read since the last publish: the position survives a reopen.
        ui.deliverable_page_scroll.insert(page, scrolled.clone());
        ui.states.insert(key, deliverable_state(100, Some(200)));
        ui.forget_unread_deliverable_scroll(key, Uuid::nil());
        assert!(ui.deliverable_page_scroll.contains_key(&page));
    }

    /// A published plan document (`plan_id`) belongs to its planning
    /// flow, so the Deliverables library skips it under every filter
    /// while ordinary records keep listing.
    #[test]
    fn plan_document_deliverables_stay_out_of_the_library() {
        let mut state = deliverable_state(100, None);
        let plan_id = Uuid::new_v4();
        let ordinary = state.deliverables[0].clone();
        let mut plan_live = ordinary.clone();
        plan_live.id = Uuid::new_v4();
        plan_live.plan_id = Some(plan_id);
        let mut plan_pinned = plan_live.clone();
        plan_pinned.id = Uuid::new_v4();
        plan_pinned.pinned_at = Some(100);
        let mut plan_dormant = plan_live.clone();
        plan_dormant.id = Uuid::new_v4();
        plan_dormant.dormant_at = Some(100);
        let mut plan_archived = plan_live.clone();
        plan_archived.id = Uuid::new_v4();
        plan_archived.archived_at = Some(100);
        let mut ordinary_archived = ordinary.clone();
        ordinary_archived.id = Uuid::new_v4();
        ordinary_archived.archived_at = Some(100);
        let plan_owned = [
            plan_live.id,
            plan_pinned.id,
            plan_dormant.id,
            plan_archived.id,
        ];
        state.deliverables = vec![
            plan_live,
            plan_pinned,
            plan_dormant,
            plan_archived,
            ordinary.clone(),
            ordinary_archived.clone(),
        ];

        for filter in [
            BossDeliverablesFilter::All,
            BossDeliverablesFilter::Pinned,
            BossDeliverablesFilter::Dormant,
            BossDeliverablesFilter::Archived,
        ] {
            let rows = boss_deliverable_rows(&state, filter);
            for id in plan_owned {
                assert!(!rows.contains(&BossItem::Deliverable(id)));
            }
        }
        assert_eq!(
            boss_deliverable_rows(&state, BossDeliverablesFilter::All),
            vec![BossItem::Deliverable(ordinary.id)]
        );
        assert_eq!(
            boss_deliverable_rows(&state, BossDeliverablesFilter::Archived),
            vec![BossItem::Deliverable(ordinary_archived.id)]
        );
    }

    fn boss_employee(created_at: Option<u64>) -> waku_protocol::boss::BossEmployee {
        waku_protocol::boss::BossEmployee {
            session_id: Uuid::new_v4(),
            supervisor_id: Uuid::new_v4(),
            identity: BossIdentity {
                id: Uuid::new_v4(),
                name: String::new(),
                avatar_seed: String::new(),
                avatar_style: Default::default(),
            },
            job_title: String::new(),
            persona_id: Uuid::new_v4(),
            work_goal: waku_protocol::boss::EmployeeGoal::Errand,
            created_at,
            icon: None,
            permissions: PersonaPermissions::default(),
            pinned_files: Vec::new(),
            expired: false,
            workspace_transition: false,
            expired_at: None,
            blocker: None,
            cancelled: false,
            expiry: None,
            state: waku_protocol::boss::EmployeeLifecycle::Working,
            ticket: None,
            queued_at: None,
            request_id: None,
            request_fingerprint: None,
            plan_id: None,
            item_id: None,
            assignment: None,
        }
    }

    #[test]
    fn resume_action_keys_off_expiry_and_the_wire_resumable_flag() {
        use waku_protocol::boss::{EmployeeExpiry, EmployeeLifecycle, ExpiryCause};
        // A live record takes a prompt, not a revive — no control at all.
        let mut employee = boss_employee(Some(100));
        assert_eq!(employee_resume_action(&employee), EmployeeResume::Hidden);

        // A supervisor stop expires the record; every cause offers the
        // action, stopped included.
        employee.set_lifecycle(EmployeeLifecycle::Expired, 200);
        employee.expiry = Some(EmployeeExpiry {
            cause: ExpiryCause::Stopped,
            resumable: true,
            parked_prompts: 0,
            pending_question: None,
        });
        assert_eq!(employee_resume_action(&employee), EmployeeResume::Enabled);

        // An expiry flagged unresumable keeps the control visible but
        // disabled rather than silently absent.
        employee.expiry.as_mut().unwrap().resumable = false;
        assert_eq!(employee_resume_action(&employee), EmployeeResume::Disabled);

        // A record too old to carry an expiry still offers the action.
        employee.expiry = None;
        assert_eq!(employee_resume_action(&employee), EmployeeResume::Enabled);
    }

    #[test]
    fn employees_sort_newest_summon_first_regardless_of_lifecycle() {
        let oldest = boss_employee(Some(100));
        let mut finished = boss_employee(Some(200));
        finished.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 250);
        let mut queued = boss_employee(Some(300));
        queued.state = waku_protocol::boss::EmployeeLifecycle::Queued;
        // A queued summon with no session yet still tops the list, and a
        // finished employee holds the position its summon earned.
        let mut employees = vec![&oldest, &queued, &finished];
        sort_boss_employees(&mut employees, &HashMap::new());
        assert_eq!(
            employees
                .iter()
                .map(|employee| employee.session_id)
                .collect::<Vec<_>>(),
            vec![queued.session_id, finished.session_id, oldest.session_id]
        );
    }

    #[test]
    fn employees_without_a_summon_stamp_fall_back_to_session_creation() {
        let legacy = boss_employee(None);
        let stamped = boss_employee(Some(200));
        let unknown = boss_employee(None);
        let session_created_at = HashMap::from([(legacy.session_id, 300)]);
        let mut employees = vec![&stamped, &legacy, &unknown];
        sort_boss_employees(&mut employees, &session_created_at);
        assert_eq!(
            employees
                .iter()
                .map(|employee| employee.session_id)
                .collect::<Vec<_>>(),
            vec![legacy.session_id, stamped.session_id, unknown.session_id]
        );
    }

    #[test]
    fn avatar_buckets_round_up_to_eights() {
        assert_eq!(avatar_bucket(1.0), 8);
        assert_eq!(avatar_bucket(16.0), 16);
        assert_eq!(avatar_bucket(18.0), 24);
        assert_eq!(avatar_bucket(54.0), 56);
    }

    #[test]
    fn gaze_eyes_sit_at_rest_outside_the_look_window() {
        // Phase 0 is the rest pose — the frame a reduce-motion render and a
        // non-leasing tick both have to match.
        for phase in [0.0, 0.08, 0.15, 0.95, 0.999, 1.0, 7.05] {
            assert_eq!(
                gaze_look_offset(phase, 24.0),
                (0.0, 0.0),
                "phase {phase}"
            );
        }
    }

    #[test]
    fn gaze_eyes_drift_and_settle_inside_the_look_window() {
        // Mid-glance holds the track's keyframed extremes; the ease between
        // holds stays inside them.
        let left = gaze_look_offset(0.3, 24.0);
        assert!(
            (left.0 - -3.6 * 0.24).abs() < 0.001,
            "phase 0.3 holds the left glance, got {left:?}"
        );
        let right = gaze_look_offset(0.5, 24.0);
        assert!(
            (right.0 - 3.4 * 0.24).abs() < 0.001,
            "phase 0.5 holds the right glance, got {right:?}"
        );
        for step in 0..100 {
            let phase = step as f32 / 100.0;
            let (dx, dy) = gaze_look_offset(phase, 24.0);
            assert!(
                dx.abs() <= 3.6 * 0.24 + f32::EPSILON && dy.abs() <= 1.7 * 0.24 + f32::EPSILON,
                "phase {phase} stays inside the glances, got ({dx}, {dy})"
            );
        }
    }

    #[test]
    fn gaze_blink_closes_only_inside_its_window() {
        // Phase 0 is the open pose — the frame a reduce-motion render has to
        // match.
        assert!(!gaze_blink_closed(0.0));
        assert!(!gaze_blink_closed(0.939));
        assert!(gaze_blink_closed(0.94));
        assert!(gaze_blink_closed(0.955));
        assert!(gaze_blink_closed(0.97));
        assert!(!gaze_blink_closed(0.971));
        assert!(!gaze_blink_closed(4.3));
    }

    #[test]
    fn gaze_eye_shift_stays_in_phase_and_is_deterministic() {
        for seed in ["boss", "zadie", "", "existing"] {
            let shift = gaze_eye_shift(seed);
            assert!((0.0..1.0).contains(&shift.0), "{seed} look at {}", shift.0);
            assert!((0.0..1.0).contains(&shift.1), "{seed} blink at {}", shift.1);
            assert_eq!(shift, gaze_eye_shift(seed));
        }
    }

    #[test]
    fn only_gaze_avatars_animate_and_never_under_reduce_motion() {
        for style in [
            AvatarStyle::DiceBear,
            AvatarStyle::LineFace,
            AvatarStyle::AgentAvatars,
            AvatarStyle::Avvvatars,
        ] {
            assert!(!gaze_animates(style, false), "{style:?} keeps its static raster");
        }
        assert!(gaze_animates(AvatarStyle::Gaze, false));
        assert!(!gaze_animates(AvatarStyle::Gaze, true));
    }

    #[test]
    fn avatar_style_setting_targets_the_global_boss_preference() {
        assert!(matches!(
            global_avatar_style_operation(AvatarStyle::LineFace),
            BossOperation::SetAvatarStyle {
                session_id: None,
                avatar_style: AvatarStyle::LineFace,
            }
        ));
    }

    #[test]
    fn failed_avatar_retries_are_bounded_and_stay_deduplicated() {
        let ui = BossUi::default();
        let key = (("failed".to_string(), AvatarStyle::LineFace), 24);
        ui.avatar_requested.borrow_mut().insert(key.clone());
        for attempt in 1..=AVATAR_MAX_ATTEMPTS {
            ui.retry_avatar(key.0.clone(), key.1, attempt);
            assert!(!ui.avatar_requested.borrow_mut().insert(key.clone()));
            let queued = ui.avatar_queue.borrow_mut().pop_front();
            if attempt < AVATAR_MAX_ATTEMPTS {
                assert_eq!(queued, Some((key.0.0.clone(), key.0.1, key.1, attempt + 1)));
            } else {
                assert!(queued.is_none(), "permanent failures must stop retrying");
            }
        }
        assert!(ui.avatar_queue.borrow().is_empty());
    }

    #[gpui::test]
    fn evicted_avatars_requeue_instead_of_keeping_their_fallback(cx: &mut gpui::TestAppContext) {
        let renderer = cx.update(|cx| cx.svg_renderer());
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#ffffff"/></svg>"##;
        let image = renderer.render_single_frame(svg.as_bytes(), 1.0).unwrap();
        let mut ui = BossUi::default();
        // More identities than the raster budget exercises the eviction
        // path without relying on which HashMap entry gets evicted.
        for id in 0..(AVATAR_CACHE_LIMIT + 64) {
            let seed = (id.to_string(), AvatarStyle::DiceBear);
            ui.avatar_requested.borrow_mut().insert((seed.clone(), 24));
            let _ = ui.cache_avatar(
                seed,
                24,
                AvatarFaces {
                    still: image.clone(),
                    gaze: None,
                },
            );
        }
        assert_eq!(
            ui.avatars.values().map(HashMap::len).sum::<usize>(),
            AVATAR_CACHE_LIMIT
        );
        assert!((0..(AVATAR_CACHE_LIMIT + 64)).any(|id| {
            !ui.avatars
                .contains_key(&(id.to_string(), AvatarStyle::DiceBear))
        }));
        // Cached keys keep their dedup mark — a hit never reaches it — while
        // evicted keys lose theirs so the next request requeues the render.
        for id in 0..(AVATAR_CACHE_LIMIT + 64) {
            let seed = (id.to_string(), AvatarStyle::DiceBear);
            if let Some(cached) = ui.avatars.get(&seed).and_then(|buckets| buckets.get(&24)) {
                assert!(Arc::ptr_eq(&cached.still, &image));
                assert!(!ui.avatar_requested.borrow_mut().insert((seed, 24)));
            } else {
                assert!(ui.avatar_requested.borrow_mut().insert((seed, 24)));
            }
        }
        assert!(
            ui.avatar_requested
                .borrow_mut()
                .insert((("new-seed".into(), AvatarStyle::DiceBear), 24))
        );
        assert!(
            ui.avatar_requested
                .borrow_mut()
                .insert((("0".into(), AvatarStyle::DiceBear), 56))
        );
    }

    /// The scale handed to the SVG rasterizer must land each bucket's
    /// raster on twice its logical size — GPUI multiplies by its smooth-SVG
    /// factor of 2, so drift here reintroduces the avatar aliasing the
    /// buckets exist to prevent.
    #[gpui::test]
    fn avatar_rasters_render_at_twice_their_bucket(cx: &mut gpui::TestAppContext) {
        let renderer = cx.update(|cx| cx.svg_renderer());
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="256" height="256"><rect width="100" height="100" fill="#ffffff"/></svg>"##;
        for bucket in [16u32, 24, 56] {
            let image = renderer
                .render_single_frame(svg.as_bytes(), avatar_scale(bucket))
                .unwrap();
            let frame = image.size(0);
            assert_eq!(frame.width.0, bucket as i32 * 2);
            assert_eq!(frame.height.0, bucket as i32 * 2);
        }
    }

    fn ticket(
        sequence: u64,
        blocked_by: Vec<waku_protocol::boss::AdmissionBlocker>,
    ) -> waku_protocol::boss::SummonTicket {
        waku_protocol::boss::SummonTicket {
            sequence,
            generation: 1,
            provider: ProviderKind::Codex,
            model: "swe-2".into(),
            reasoning_effort: None,
            prompt: "do it".into(),
            project: "/tmp".into(),
            workspace: None,
            base_branch: None,
            adopt_worktree: None,
            resources: waku_protocol::resources::ResourceSet::default(),
            allow_burst: false,
            pending_prompts: Vec::new(),
            group_id: None,
            priority: None,
            outcome_id: None,
            reservation: None,
            pending_resources: None,
            pending_reservation: None,
            blocked_by,
            dispatch_event: None,
            interruptions: Vec::new(),
            resume_count: 0,
            last_resumed_cause: None,
        }
    }

    fn employee(
        id: u128,
        lifecycle: waku_protocol::boss::EmployeeLifecycle,
        ticket: Option<waku_protocol::boss::SummonTicket>,
    ) -> waku_protocol::boss::BossEmployee {
        let session_id = Uuid::from_u128(id);
        waku_protocol::boss::BossEmployee {
            session_id,
            supervisor_id: Uuid::from_u128(u128::MAX),
            identity: BossIdentity {
                id: session_id,
                name: format!("Employee {id}"),
                avatar_seed: String::new(),
                avatar_style: Default::default(),
            },
            job_title: "Tester".into(),
            persona_id: Uuid::from_u128(u128::MAX - 1),
            work_goal: waku_protocol::boss::EmployeeGoal::Errand,
            created_at: None,
            icon: None,
            permissions: PersonaPermissions::default(),
            pinned_files: Vec::new(),
            expired: lifecycle == waku_protocol::boss::EmployeeLifecycle::Expired,
            workspace_transition: false,
            expired_at: None,
            blocker: None,
            cancelled: false,
            expiry: None,
            state: lifecycle,
            ticket,
            queued_at: (lifecycle == waku_protocol::boss::EmployeeLifecycle::Queued).then_some(1),
            request_id: None,
            request_fingerprint: None,
            plan_id: None,
            item_id: None,
            assignment: None,
        }
    }

    fn outcome(id: u128) -> waku_protocol::boss::BossOutcome {
        waku_protocol::boss::BossOutcome {
            id: Uuid::from_u128(id),
            outcome: "Ship the feature".into(),
            success_criteria: "checks pass".into(),
            state: waku_protocol::boss::OutcomeState::Open,
            finishing_assignment: None,
            handoffs: Vec::new(),
            completion_conflict: None,
            evidence: None,
            plan_id: None,
            waiting: None,
            snoozed_until: None,
            last_activity_at: 0,
            unattended_since: None,
            last_reminder: None,
            created_at: 0,
            completed_at: None,
            history: Vec::new(),
            assignments: Vec::new(),
        }
    }

    fn settled_row(
        session: u128,
        verdict: waku_protocol::boss::AssignmentVerdict,
        blocked: bool,
    ) -> waku_protocol::boss::OutcomeAssignment {
        waku_protocol::boss::OutcomeAssignment {
            session: Uuid::from_u128(session),
            generation: 1,
            identity: None,
            job_title: None,
            finishes_outcome: None,
            after_success: None,
            prerequisites: Vec::new(),
            assigned_at: None,
            settled: Some(waku_protocol::boss::AssignmentSettle {
                verdict,
                cause: None,
                blocked,
                at: Some(10),
            }),
        }
    }

    /// Tasks attention survives retirement: a settled failure or blocker
    /// the roster no longer covers still flags the outcome, while a
    /// rostered member's own verdict answers for it — a retried attempt
    /// reopens its row rather than leaving stale attention behind.
    #[test]
    fn outcome_attention_outlives_the_roster_record() {
        use waku_protocol::boss::{AssignmentVerdict, EmployeeLifecycle};

        // A failed attempt with no roster record left still needs attention.
        let mut task = outcome(1);
        task.assignments.push(settled_row(9, AssignmentVerdict::Failed, false));
        assert!(boss_outcome_attention(&task, &[]));

        // A flagged blocker reads as failed evidence the same way.
        task.assignments[0].settled.as_mut().unwrap().verdict =
            AssignmentVerdict::Finished;
        task.assignments[0].settled.as_mut().unwrap().blocked = true;
        assert!(boss_outcome_attention(&task, &[]));

        // Clean finishes and deliberate cancels leave nothing owed.
        task.assignments[0].settled.as_mut().unwrap().blocked = false;
        assert!(!boss_outcome_attention(&task, &[]));
        task.assignments[0].settled.as_mut().unwrap().verdict =
            AssignmentVerdict::Cancelled;
        assert!(!boss_outcome_attention(&task, &[]));
        task.assignments[0].settled.as_mut().unwrap().verdict =
            AssignmentVerdict::Superseded;
        assert!(!boss_outcome_attention(&task, &[]));

        // A rostered member's clean record answers for its own settled
        // failure — the durable row defers to it.
        let member = BossOutcomeMember {
            employee: employee(9, EmployeeLifecycle::Expired, None),
            queue_rank: None,
        };
        task.assignments[0].settled.as_mut().unwrap().verdict = AssignmentVerdict::Failed;
        assert!(!boss_outcome_attention(&task, &[member.clone()]));

        // The same member's unresolved record flags the outcome itself.
        let mut failed_member = member;
        failed_member.employee.expiry = Some(waku_protocol::boss::EmployeeExpiry {
            cause: waku_protocol::boss::ExpiryCause::Failed,
            resumable: true,
            parked_prompts: 0,
            pending_question: None,
        });
        assert!(boss_outcome_attention(&task, &[failed_member]));

        // An open completion conflict flags the outcome on its own.
        let mut conflicted = outcome(2);
        conflicted.completion_conflict = Some(waku_protocol::boss::CompletionConflict {
            assignment: Uuid::from_u128(9),
            attempt: 1,
            reason: "handoff still pending".into(),
            at: 10,
        });
        assert!(boss_outcome_attention(&conflicted, &[]));
    }

    #[test]
    fn archived_employee_session_reads_durable_state() {
        // A retired employee: off the live roster, but the `boss_managed`
        // stamp and the archive flag are the session record's own.
        let mut retired = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        retired.boss_managed = true;
        retired.archived_at = Some(1);
        assert!(archived_employee_session(
            &retired,
            &HashSet::new(),
            false,
            true
        ));

        // A rostered employee summoned before the stamp existed still
        // qualifies through `managed`.
        let mut rostered = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        rostered.archived_at = Some(1);
        let managed = HashSet::from([rostered.id]);
        assert!(archived_employee_session(&rostered, &managed, false, true));

        // Live employees that were never archived, plain tasks, and the
        // boss's own surfaces do not.
        rostered.archived_at = None;
        assert!(!archived_employee_session(&rostered, &managed, false, true));
        let mut task = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        task.archived_at = Some(1);
        assert!(!archived_employee_session(
            &task,
            &HashSet::new(),
            false,
            true
        ));
        assert!(!archived_employee_session(
            &retired,
            &HashSet::new(),
            true,
            true
        ));
        let mut planning = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        planning.boss_managed = true;
        planning.archived_at = Some(1);
        planning.planning = Some(crate::model::SessionPlanning {
            plan_file: "plans/auth.md".into(),
            idea: "Auth".into(),
            label: waku_client::WireTranslation::new("boss.planning_label", []),
            finalized_at: None,
        });
        assert!(!archived_employee_session(
            &planning,
            &HashSet::new(),
            false,
            true
        ));

        // With the experiment off the whole classification is inert.
        assert!(!archived_employee_session(
            &retired,
            &HashSet::new(),
            false,
            false
        ));
    }

    #[test]
    fn owning_state_covers_the_chat_and_its_planning_sessions() {
        let mut state = boss_state_for_queue_test();
        let chat = Uuid::new_v4();
        let planning = Uuid::new_v4();
        state.session_id = Some(chat);
        state.planning = vec![waku_protocol::boss::BossPlan {
            id: Uuid::new_v4(),
            session_id: planning,
            plan_file: "plans/auth.md".into(),
            idea: "Auth".into(),
            finalized_at: None,
            items: Vec::new(),
            outcome: None,
            history: Vec::new(),
        }];
        let states = HashMap::from([(DaemonKey::Local, state)]);

        assert_eq!(
            boss_state_owning_session(&states, chat).and_then(|state| state.session_id),
            Some(chat)
        );
        assert_eq!(
            boss_state_owning_session(&states, planning).and_then(|state| state.session_id),
            Some(chat)
        );
        assert!(boss_state_owning_session(&states, Uuid::new_v4()).is_none());
    }

    #[test]
    fn finalizing_the_viewed_plan_returns_to_its_boss_only_once() {
        let mut previous = boss_state_for_queue_test();
        let planning = Uuid::new_v4();
        previous.planning = vec![waku_protocol::boss::BossPlan {
            id: Uuid::new_v4(),
            session_id: planning,
            plan_file: "plans/auth.md".into(),
            idea: "Auth".into(),
            finalized_at: None,
            items: Vec::new(),
            outcome: None,
            history: Vec::new(),
        }];
        let mut current = previous.clone();
        assert!(!viewed_plan_just_finalized(
            Some(&previous),
            &current,
            Some(planning)
        ));
        current.planning[0].finalized_at = Some(100);
        assert!(viewed_plan_just_finalized(
            Some(&previous),
            &current,
            Some(planning)
        ));
        // Loading historical approval, refreshing it, or finalizing a plan
        // outside the viewed chat must not steal navigation.
        assert!(!viewed_plan_just_finalized(None, &current, Some(planning)));
        assert!(!viewed_plan_just_finalized(
            Some(&current),
            &current,
            Some(planning)
        ));
        assert!(!viewed_plan_just_finalized(
            Some(&previous),
            &current,
            Some(Uuid::new_v4())
        ));
        assert!(!viewed_plan_just_finalized(Some(&previous), &current, None));
        current.identity.id = Uuid::new_v4();
        assert!(!viewed_plan_just_finalized(
            Some(&previous),
            &current,
            Some(planning)
        ));
    }

    #[test]
    fn queue_ranks_follow_ticket_sequence() {
        let mut state = boss_state_for_queue_test();
        let head = employee(
            1,
            waku_protocol::boss::EmployeeLifecycle::Queued,
            Some(ticket(7, Vec::new())),
        );
        let tail = employee(
            2,
            waku_protocol::boss::EmployeeLifecycle::Queued,
            Some(ticket(3, Vec::new())),
        );
        let working = employee(3, waku_protocol::boss::EmployeeLifecycle::Working, None);
        state.employees = vec![head.clone(), tail.clone(), working.clone()];

        let ranks = boss_queue_ranks(&state);
        // The lower sequence admitted first — insertion order does not
        // matter, and working employees earn no rank.
        assert_eq!(ranks.get(&tail.session_id), Some(&1));
        assert_eq!(ranks.get(&head.session_id), Some(&2));
        assert!(!ranks.contains_key(&working.session_id));
    }

    #[test]
    fn queue_detail_reports_the_blocker_or_a_neutral_reason() {
        use waku_protocol::boss::{AdmissionBlocker, EmployeeLifecycle};
        let working = employee(1, EmployeeLifecycle::Working, None);
        assert_eq!(boss_queue_detail(&working, None), None);
        let expired = employee(2, EmployeeLifecycle::Expired, None);
        assert_eq!(boss_queue_detail(&expired, None), None);

        let limited = employee(
            3,
            EmployeeLifecycle::Queued,
            Some(ticket(
                1,
                vec![AdmissionBlocker::ModelLimit { used: 6, limit: 6 }],
            )),
        );
        assert_eq!(
            boss_queue_detail(&limited, Some(1)),
            Some(tr!(
                "boss.goals_queue_model",
                model = "swe-2",
                used = 6u32,
                limit = 6u32
            ))
        );

        let waiting = employee(4, EmployeeLifecycle::Queued, Some(ticket(1, Vec::new())));
        assert_eq!(
            boss_queue_detail(&waiting, Some(1)),
            Some(tr!("boss.goals_queue_admission"))
        );
        assert_eq!(
            boss_queue_detail(&waiting, Some(2)),
            Some(tr!("boss.goals_queue_earlier"))
        );

        let resources = employee(
            5,
            EmployeeLifecycle::Queued,
            Some(ticket(
                1,
                vec![AdmissionBlocker::HostResources {
                    detail: "waiting for a build slot".into(),
                }],
            )),
        );
        assert_eq!(
            boss_queue_detail(&resources, Some(1)),
            Some("waiting for a build slot".to_owned())
        );
    }

    fn boss_state_for_queue_test() -> BossState {
        BossState {
            identity: BossIdentity {
                id: Uuid::from_u128(u128::MAX - 2),
                name: "Boss".into(),
                avatar_seed: String::new(),
                avatar_style: Default::default(),
            },
            persona_id: Uuid::from_u128(u128::MAX - 3),
            employee_persona_id: None,
            specialist_persona_ids: Default::default(),
            persona_defaults: Default::default(),
            persona_default_notice: None,
            session_id: None,
            personas: Vec::new(),
            employees: Vec::new(),
            retired_employees: Vec::new(),
            deliverables: Vec::new(),
            goals_viewed_at: None,
            planning: Vec::new(),
            outcomes: Vec::new(),
            resource_policy: waku_protocol::boss::BossResourcePolicy::default(),
            next_sequence: 0,
            next_event_id: 0,
            name_cursor: 0,
            outbox: Vec::new(),
            waves: Vec::new(),
            wave_outbox: Vec::new(),
            revision: 0,
        }
    }

    fn history_hit(task: u8, message: u8) -> waku_protocol::model::AgentHistorySearchHit {
        waku_protocol::model::AgentHistorySearchHit {
            task_id: Uuid::from_u128(task as u128),
            kind: waku_protocol::model::HistorySourceKind::Task,
            title: format!("record {task}"),
            project: "project".into(),
            person: None,
            job_title: None,
            employee_expired: None,
            status: waku_protocol::model::SessionStatus::Idle,
            archived: true,
            created_at: 10,
            updated_at: 20,
            message_id: Uuid::from_u128(message as u128),
            role: waku_protocol::model::MessageRole::Assistant,
            recorded_by: None,
            excerpt: "the matched passage".into(),
            excerpt_matched: true,
            excerpt_at: 15,
            matched_terms: vec!["matched".into()],
            title_matched: false,
            matched_messages: 1,
        }
    }

    #[test]
    fn history_lookup_engages_on_text_or_any_filter() {
        // An empty field over default filters keeps the finished roster.
        assert!(!boss_history_search_active("", None));
        assert!(!boss_history_search_active("  ", Some(&BossHistorySearch::default())));

        // Field text alone engages the lookup — the query names what the
        // human remembers, not an employee.
        assert!(boss_history_search_active("zed upgrade", None));
        assert!(boss_history_search_active(
            "walter",
            Some(&BossHistorySearch::default())
        ));

        // Each filter alone engages it — a filtered listing needs no
        // query text.
        for search in [
            BossHistorySearch {
                project: Some(Uuid::new_v4()),
                ..Default::default()
            },
            BossHistorySearch {
                kind: Some(waku_protocol::model::HistorySourceKind::Boss),
                ..Default::default()
            },
            BossHistorySearch {
                range: HistoryDateRange::Last30,
                ..Default::default()
            },
        ] {
            assert!(boss_history_search_active("", Some(&search)));
        }
    }

    #[test]
    fn history_rows_pair_hits_with_the_coverage_footer() {
        // An unresolved lookup draws no rows — the body shows its own
        // searching/error/no-match states.
        assert!(boss_history_rows(&BossHistorySearch::default()).is_empty());
        assert!(
            boss_history_rows(&BossHistorySearch {
                searching: true,
                ..Default::default()
            })
                == vec![BossItem::HistoryFooter]
        );

        // Hits list in order; the footer follows while anything —
        // coverage, an in-flight page, or an error — stays reportable.
        let resolved = BossHistorySearch {
            hits: vec![history_hit(1, 1), history_hit(2, 2)],
            coverage: Some(waku_protocol::model::AgentHistorySearchCoverage {
                scope: "every project".into(),
                includes_archived: true,
                kinds: Vec::new(),
                sources_scanned: 4,
                sources_matched: 2,
                returned: 2,
                truncated: false,
                next_offset: None,
                excluded_by_access: 0,
                notes: Vec::new(),
            }),
            ..Default::default()
        };
        assert_eq!(
            boss_history_rows(&resolved),
            vec![
                BossItem::HistoryHit(0),
                BossItem::HistoryHit(1),
                BossItem::HistoryFooter
            ]
        );

        // A failed lookup still lists the hits it already had.
        let failed = BossHistorySearch {
            hits: vec![history_hit(1, 1)],
            error: Some("the daemon is unreachable".into()),
            ..Default::default()
        };
        assert_eq!(
            boss_history_rows(&failed),
            vec![BossItem::HistoryHit(0), BossItem::HistoryFooter]
        );
    }

    #[test]
    fn history_continuation_dedupes_reranked_records() {
        let mut hits = vec![history_hit(1, 1), history_hit(2, 2)];
        merge_history_page(
            &mut hits,
            // A source re-ranked between pages repeats; a new message on
            // the same source does not.
            vec![history_hit(2, 2), history_hit(2, 9), history_hit(3, 3)],
        );
        let ids: Vec<(Uuid, Uuid)> = hits
            .iter()
            .map(|hit| (hit.task_id, hit.message_id))
            .collect();
        assert_eq!(
            ids,
            vec![
                (Uuid::from_u128(1), Uuid::from_u128(1)),
                (Uuid::from_u128(2), Uuid::from_u128(2)),
                (Uuid::from_u128(2), Uuid::from_u128(9)),
                (Uuid::from_u128(3), Uuid::from_u128(3)),
            ]
        );
    }

    #[test]
    fn history_hit_ids_stay_unique_per_passage() {
        // Two passages from one record keep distinct row ids, and the id
        // is stable enough for keyboard focus to survive a refresh.
        let task = Uuid::from_u128(7);
        assert_eq!(
            boss_history_hit_id(task, Uuid::from_u128(1)),
            boss_history_hit_id(task, Uuid::from_u128(1))
        );
        assert_ne!(
            boss_history_hit_id(task, Uuid::from_u128(1)),
            boss_history_hit_id(task, Uuid::from_u128(2))
        );
        assert_ne!(
            boss_history_hit_id(Uuid::from_u128(8), Uuid::from_u128(1)),
            boss_history_hit_id(task, Uuid::from_u128(1))
        );
    }

    #[test]
    fn history_date_ranges_bound_by_message_age() {
        let now = 100 * HistoryDateRange::DAY;
        assert_eq!(HistoryDateRange::Any.bounds(now), (None, None));
        assert_eq!(
            HistoryDateRange::Last7.bounds(now),
            (Some(93 * HistoryDateRange::DAY), None)
        );
        assert_eq!(
            HistoryDateRange::Last90.bounds(now),
            (Some(10 * HistoryDateRange::DAY), None)
        );
        // "Older" is the complement — everything before the 90-day window.
        assert_eq!(
            HistoryDateRange::Before90.bounds(now),
            (None, Some(10 * HistoryDateRange::DAY))
        );
        // The window clamps at zero rather than wrapping.
        assert_eq!(HistoryDateRange::Last7.bounds(0), (Some(0), None));
    }
}
