//! Desktop control plane for daemon-owned Boss roles.
use super::annotations::{
    annotation_bubble_content, annotation_display_content, annotation_prompt_prefix,
};
use super::boss_moods::AVATAR_SOURCE_SIZE;
use super::sessions::session_archivable;
use super::*;
use crate::ui::ActivationExt;
use waku_client::DaemonKey;
use waku_client::boss::{
    BossEmployee, BossFile, BossIdentity, BossOperation, BossPersona, BossPersonaUpsert,
    BossResult, BossState, MemoryOperation, PersonaDefaultAction, PersonaDefaultRole,
    PersonaInstructionLayer, PersonaPermissions,
    instruction_diff, shipped_persona_default, shipped_persona_prefix, shipped_persona_revision,
};
use waku_protocol::boss::AvatarStyle;
use waku_protocol::custom_commands::CustomCommandIcon;

fn global_avatar_style_operation(avatar_style: AvatarStyle) -> BossOperation {
    BossOperation::SetAvatarStyle {
        session_id: None,
        avatar_style,
    }
}

fn employee_attention_is_unread(employee: &BossEmployee) -> bool {
    employee.needs_attention
        && employee.expired
        && employee.attention_viewed_at.is_none_or(|viewed_at| {
            employee
                .expired_at
                .is_some_and(|expired_at| viewed_at < expired_at)
        })
}

/// The source label heading one instruction layer — "Inherited —
/// Employee", "Shipped — Researcher", or "Custom".
fn persona_layer_label(layer: &PersonaInstructionLayer) -> String {
    match layer {
        PersonaInstructionLayer::Inherited { name, .. } => {
            tr!("boss.layer_inherited", name = name.clone())
        }
        PersonaInstructionLayer::Shipped { role, .. } => {
            tr!("boss.layer_shipped", name = role.label())
        }
        PersonaInstructionLayer::Custom { .. } => tr!("boss.layer_custom"),
    }
}

/// The persona detail's instruction document: one bullet per effective
/// layer in the agent's composed order — inherited Employee base, the
/// shipped default, the appended customization — each labeled by source
/// with its text nested beneath the bullet so the layering is scannable.
fn persona_layers_document(layers: &[PersonaInstructionLayer]) -> String {
    let mut document = String::new();
    for layer in layers {
        document.push_str("- **");
        document.push_str(&persona_layer_label(layer));
        document.push_str("**\n");
        for line in layer.markdown().lines() {
            document.push('\n');
            if !line.trim().is_empty() {
                document.push_str("  ");
                document.push_str(line);
            }
        }
        document.push('\n');
    }
    document
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
pub(super) struct GazeFaces {
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
pub(super) struct MemoryFeedRecord {
    /// The note's bucket-local stable id — `"{sequence}-{digest}"`.
    pub(super) id: String,
    pub(super) bucket_id: String,
    /// The owning bucket's display name — a provenance label, not a filter.
    pub(super) bucket: String,
    pub(super) sequence: u64,
    pub(super) text: String,
    pub(super) created_at: u64,
    /// `None` — the bucket is confirmed to have no project association, the
    /// feed's "No project" grouping.
    pub(super) project: Option<MemoryFeedProject>,
}

/// The record's repository association, keyed on the project bucket id so
/// every worktree of one repository collapses to a single project choice.
/// `name` resolves through the project catalog — `None` when nothing does,
/// the feed's "Unknown project" grouping.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MemoryFeedProject {
    pub(super) key: String,
    pub(super) name: Option<String>,
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
pub(super) struct MemoryFeed {
    pub(super) records: Vec<MemoryFeedRecord>,
    /// A response has landed — distinguishes "loading" from "failed before
    /// anything arrived".
    pub(super) loaded: bool,
    /// The daemon clock time the last applied feed was fetched — a
    /// transcript memory card only trusts "absent means removed" when the
    /// feed is this fresh.
    refreshed_at: Option<u64>,
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
            refreshed_at: None,
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
pub(super) fn memory_record_age(seconds: u64) -> String {
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
    /// The Goals panel's single virtualized scroll surface. Fold and history
    /// choices are honored per Boss daemon across snapshots.
    pub(super) goals_list: ListState,
    pub(super) goals_scrollbar: Rc<ScrollbarState>,
    pub(super) goals_list_owner: Option<DaemonKey>,
    pub(super) goals_signature: Option<u64>,
    pub(super) goals_collapsed: HashSet<(DaemonKey, BossGoalSection)>,
    /// Daemons whose Pending section was seeded collapsed — the fold is a
    /// first-visit default; toggles after that win freely.
    pub(super) goals_collapsed_seeded: HashSet<DaemonKey>,
    /// Sections whose age-folded rows the user revealed — honored per Boss
    /// daemon and section across snapshots.
    pub(super) goals_history_expanded: HashSet<(DaemonKey, BossGoalSection)>,
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
    /// the slot instead of stranding the text on the archived row, and
    /// submissions armed before the swap retarget to the live chat.
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
    pub employee_purposes: HashMap<Uuid, waku_protocol::boss::EmployeePurpose>,
    pub expired: HashSet<Uuid>,
    /// Employees wearing the user-attention unread marker right now:
    /// flagged at summon or by hand, expired, and not viewed since that
    /// expiry. Rebuilt with every Boss snapshot; the row reads membership
    /// here instead of rescanning employee records while rendering.
    pub attention: HashSet<Uuid>,
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
    /// A `startNow` press the daemon has not answered or broadcast yet — the
    /// queued card commits to "Starting" on this alone so a second click can
    /// never double-send. Entries retire on the reply and on the next state
    /// that either carries the force flag or drops the ticket's queue seat.
    pub(super) start_now_pending: HashSet<Uuid>,
    pub recent: HashMap<DaemonKey, Vec<Uuid>>,
    pub sidebar_idle_visible: HashMap<DaemonKey, usize>,
    /// The Errands group's own finished-employee reveal count — the same
    /// "name finished ago" fold as `sidebar_idle_visible`, kept per section
    /// so one group's expansion never reveals the other's tail.
    pub sidebar_errand_idle_visible: HashMap<DaemonKey, usize>,
    /// Legacy Employees keeps its own finished-tail reveal counter.
    pub sidebar_employee_idle_visible: HashMap<DaemonKey, usize>,
    /// The deliverable a sidebar click armed the composer with: the next main-
    /// composer submission commands the boss with this file attached.
    /// Cleared by a send or any landing that is not re-arming it — the
    /// `pending_deliverable` half carries it across its own navigation.
    pub command_deliverable: Option<(DaemonKey, Uuid)>,
    /// Per-daemon destination picks off the composer's target chip: the
    /// employee session id sends land on directly, keyed by the boss that
    /// owns the employee so a pick under one boss never reroutes
    /// another's. Absent means the armed command's default — the boss
    /// chat. Session ids keep a pick stable across roster reorders.
    pub(super) composer_targets: HashMap<DaemonKey, Uuid>,
    /// Explicit Boss -> Employee switches made inside that employee's chat.
    pub(super) employee_composer_interactions: HashMap<DaemonKey, Uuid>,
    /// Keep the submission identity through a lost reply and restored draft.
    pub(super) employee_composer_submissions: HashMap<Uuid, (Uuid, String, waku_client::AgentPromptDelivery)>,
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
    persona_selection: TranscriptSelection,
    persona_scroll: ScrollHandle,
    persona_scrollbar: Rc<ScrollbarState>,
    /// Per-persona expansion for the shipped-default card — comparison,
    /// shipped text, and the reset confirmation, keyed by persona id.
    persona_default_pane: HashMap<Uuid, PersonaDefaultPane>,
    persona_delete_confirmation: Option<(DaemonKey, Uuid)>,
    /// The shipped↔saved diff text for an expanded comparison, keyed to
    /// the markdown fingerprint it was computed from.
    persona_diff: RefCell<Option<(Uuid, u64, String)>>,
    /// The unified Memory Records feed per host — records, browse state,
    /// and the pending refresh a visit re-checks for.
    pub(super) memory_feed: HashMap<DaemonKey, MemoryFeed>,
    /// The feed's project choice per host — absent means All projects.
    memory_project: HashMap<DaemonKey, MemoryProjectFilter>,
    /// The `boss_memory_search` field's current text, mirrored on Edited so
    /// row building never touches the input entity.
    pub(super) memory_query: String,
    /// A `Feed` operation parked while another request held the pipe — a
    /// refresh issued mid-flight replaces it rather than dropping. Last
    /// submission wins.
    queued_memory_feed: Option<(DaemonKey, BossOperation)>,
    /// `Retract` operations parked the same way, in click order — unlike a
    /// feed refresh every undo must reach the daemon, so they queue
    /// rather than collapsing.
    queued_memory_retract: VecDeque<(DaemonKey, BossOperation)>,
    /// The `(daemon, bucket, note)` identities of retracts issued but not
    /// yet answered — replies pop in submission order since boss requests
    /// serialize through `pending`.
    pending_memory_retracts: VecDeque<(DaemonKey, String, String)>,
    /// Notes confirmed removed this run — a transcript card whose marker
    /// the daemon could not settle still reads removed.
    retracted_memory: HashSet<(DaemonKey, String, String)>,
    editor: Option<BossEditor>,
    generation: u64,
    pending: bool,
    pending_reply: Option<BossReply>,
    /// The session that was selected when the in-flight `BossOperation::Open`
    /// went out — the surface the open was asked to navigate away from. Its
    /// reply may activate the chat only while this still holds; a picker
    /// commit or a sidebar click in between retires the intent.
    open_from: Option<Uuid>,
    /// Planning sessions whose `finalizePlan` dispatch went out and did
    /// not come back failed — the approval chip hides and the composer
    /// seals for the in-flight window. Success leaves the entry until
    /// `finalized_at` lands on the session so neither flickers back; a
    /// failed reply removes it so both restore for a retry.
    pub(super) plan_finalizing: HashSet<Uuid>,
    /// An `Open` a boss-chat click carried while another request held the
    /// pipe, parked with the surface it was issued from. The next request
    /// completion re-issues it so the daemon still sees the open — unless
    /// the user has since left that surface, which retires the intent.
    /// Last click wins.
    queued_open: Option<(DaemonKey, BossOperation, Option<Uuid>)>,
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
            goals_list: ListState::new(0, ListAlignment::Top, px(640.0)),
            goals_scrollbar: ScrollbarState::new(),
            goals_list_owner: None,
            goals_signature: None,
            goals_collapsed: HashSet::new(),
            goals_collapsed_seeded: HashSet::new(),
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
            employee_purposes: HashMap::new(),
            expired: HashSet::new(),
            attention: HashSet::new(),
            workspace_transitions: HashSet::new(),
            queued: HashMap::new(),
            dispatching: HashSet::new(),
            queued_model_targets: HashMap::new(),
            start_now_pending: HashSet::new(),
            recent: HashMap::new(),
            sidebar_idle_visible: HashMap::new(),
            sidebar_errand_idle_visible: HashMap::new(),
            sidebar_employee_idle_visible: HashMap::new(),
            command_deliverable: None,
            composer_targets: HashMap::new(),
            employee_composer_interactions: HashMap::new(),
            employee_composer_submissions: HashMap::new(),
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
            persona_selection: TranscriptSelection::default(),
            persona_scroll: ScrollHandle::new(),
            persona_scrollbar: ScrollbarState::new(),
            persona_default_pane: HashMap::new(),
            persona_delete_confirmation: None,
            persona_diff: RefCell::new(None),
            memory_feed: HashMap::new(),
            memory_project: HashMap::new(),
            memory_query: String::new(),
            queued_memory_feed: None,
            queued_memory_retract: VecDeque::new(),
            pending_memory_retracts: VecDeque::new(),
            retracted_memory: HashSet::new(),
            editor: None,
            generation: 0,
            pending: false,
            pending_reply: None,
            open_from: None,
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
    Pending,
    Blocked,
    InProgress,
    Finished,
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

/// The employee card's prepared face — the Gaze eye layers when that
/// style animates, else the still raster or the placeholder's initial.
/// Rasters resolve while the caller holds the view so a virtualized
/// list's item builder can paint without re-leasing it.
#[derive(Clone)]
pub(super) enum BossEmployeeAvatar {
    Gaze {
        faces: Arc<GazeFaces>,
        shift: (f32, f32),
    },
    Still {
        image: Option<Arc<gpui::RenderImage>>,
        circular: bool,
        initial: SharedString,
    },
}

/// The reason a card shows on its own line — a reported blocker, an
/// admission wait, or a recorded cause. Reasons never squeeze into the
/// job line, where they truncated into a fragment that read as broken.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum BossEmployeeReason {
    /// The employee's own reported blocker, or a settled blocked flag.
    Blocked(String),
    /// A queued admission's recorded wait.
    Queued(String),
    /// A recorded cause — a settle's finer-grained expiry reason.
    Detail(String),
}

impl BossEmployeeReason {
    pub(super) fn text(&self) -> &str {
        match self {
            Self::Blocked(text) | Self::Queued(text) | Self::Detail(text) => text,
        }
    }

    fn color(&self, theme: &Theme) -> Hsla {
        match self {
            Self::Blocked(_) => theme.warning,
            Self::Queued(_) => theme.text_secondary,
            Self::Detail(_) => theme.text_tertiary,
        }
    }
}

/// A required-plan chip's resolved state — the bound plan's open target
/// and whether the blocked retry shows. Resolved beside the row so the
/// shared card builds the chips from a weak handle.
#[derive(Clone, Copy)]
pub(super) struct BossEmployeePlanRecovery {
    /// The bound plan's session while it still resolves — the Open chip's
    /// target.
    pub open: Option<Uuid>,
    /// The required plan's review is blocked — the Retry chip shows.
    pub retry: bool,
}

/// One employee row's prepared paint data — the Employees tab and an
/// expanded outcome's member list render through the same
/// `boss_employee_row_element`, so the two surfaces cannot drift.
pub(super) struct BossEmployeeRow {
    /// Element-id slot — "page" on the Employees tab, the outcome's
    /// `outcome:session:generation` entry key inside an expanded row —
    /// keeps the id unique when one employee shows on both surfaces.
    pub slot: String,
    /// The row's activation destination and chip target.
    pub session_id: Uuid,
    /// The daemon whose boss owns the record — chip dispatches go here.
    pub key: DaemonKey,
    pub avatar: BossEmployeeAvatar,
    pub name: String,
    /// Employee rows keep the job title here; outcome rows use this for
    /// attempt and finisher context without repeating the job title.
    pub detail: String,
    pub job_icon: &'static str,
    pub project: Option<String>,
    /// The right-aligned relative stamp, resolved against `now`.
    pub stamp: Option<u64>,
    pub now: u64,
    /// The blocked/queued/cause lines under the job detail — one line
    /// each so the full text reads instead of truncating inline.
    pub reasons: Vec<BossEmployeeReason>,
    pub readiness: Option<String>,
    pub status: Option<(String, Hsla)>,
    /// The bound plan's recovery chips — `None` for unbound employees.
    pub plan_recovery: Option<BossEmployeePlanRecovery>,
    /// Virtualized rows keep a persistent focus handle so a remount
    /// across sections holds focus; the page list's tab stop is `None`.
    pub focus: Option<FocusHandle>,
    /// Rows without a resolvable session stay inert — no pointer, tab
    /// stop, or activation.
    pub navigable: bool,
    /// Nested under an outcome row — indented past its marker column.
    pub nested: bool,
    pub aria: Option<String>,
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
/// is on screen, the deliverable a sidebar click armed, or the pending
/// memory-feedback batch.
pub(super) enum BossCommandContext {
    Employee(Uuid),
    /// Pending memory notes fold into the submission as annotations for
    /// this daemon's boss — the daemon the records belong to, needed to
    /// drain only that host's batch.
    MemoryFeedback(DaemonKey),
    Deliverable {
        path: PathBuf,
        name: String,
        directory: bool,
    },
}

/// Where the main composer lands a submission while a boss command is
/// armed: the boss chat by default, or the employee the destination chip
/// picked. A `Stale` pick names an employee that left its boss's roster —
/// sends on it refuse rather than silently rerouting to the boss the
/// draft was not written for.
pub(super) enum ComposerTarget {
    Boss,
    Employee(Uuid),
    Stale { employee: Uuid, name: String },
}

/// What a stored composer-target pick resolves to against the roster it
/// was picked from: `Unpicked` keeps the boss chat as the destination,
/// `Live` routes the send to the employee, `Stale` marks a pick whose
/// employee left the roster so callers refuse rather than reroute
/// silently.
pub(super) enum ComposerTargetPick {
    Unpicked,
    Live(Uuid),
    Stale(Uuid),
}

/// Admission intent requires both the explicit switch and the current viewed
/// employee. A pick from the Boss page or another chat cannot inherit it.
pub(super) fn employee_composer_interaction(
    context: &BossCommandContext,
    selected: Option<Uuid>,
    target: Option<Uuid>,
    switched: bool,
) -> bool {
    switched && matches!(context, BossCommandContext::Employee(viewed)
        if selected == Some(*viewed) && target == Some(*viewed))
}

impl BossUi {
    /// Only a restored failed draft reuses its receipt. Two fresh sends of
    /// identical text are distinct user requests, even while one is in flight.
    pub(super) fn employee_composer_submission(
        &mut self,
        employee: Uuid,
        prompt: &str,
        delivery: waku_client::AgentPromptDelivery,
    ) -> Uuid {
        self.employee_composer_submissions.remove(&employee)
            .filter(|stored| stored.1 == prompt && stored.2 == delivery)
            .map(|stored| stored.0)
            .unwrap_or_else(Uuid::new_v4)
    }

    pub(super) fn restore_employee_composer_submission(
        &mut self,
        employee: Uuid,
        id: Uuid,
        prompt: String,
        delivery: waku_client::AgentPromptDelivery,
    ) {
        self.employee_composer_submissions.insert(employee, (id, prompt, delivery));
    }

    pub(super) fn accept_employee_composer_submission(&mut self, employee: Uuid, id: Uuid) {
        if self.employee_composer_submissions.get(&employee).is_some_and(|stored| stored.0 == id) {
            self.employee_composer_submissions.remove(&employee);
        }
    }

    /// Resolve the daemon's stored chip pick against its current roster.
    /// A session id keeps a pick stable across reorders; membership in the
    /// roster (expired employees stay listed) keeps it live, anything
    /// else — retired, unrostered, no roster received yet — reads stale.
    fn composer_target_pick(&self, key: DaemonKey) -> ComposerTargetPick {
        let Some(employee) = self.composer_targets.get(&key).copied() else {
            return ComposerTargetPick::Unpicked;
        };
        match self.recent.get(&key) {
            Some(roster) if roster.contains(&employee) => ComposerTargetPick::Live(employee),
            _ => ComposerTargetPick::Stale(employee),
        }
    }
}

/// The roster pick an armed command honors. An employee chat's composer
/// names only two destinations — the boss and the employee the chat
/// belongs to — so a pick made for a colleague elsewhere resolves
/// unpicked here rather than retargeting the draft onto them. The chat's
/// own employee keeps its pick, live or stale; every other armed context
/// keeps the whole roster's.
fn composer_target_pick_for_context(
    context: &BossCommandContext,
    pick: ComposerTargetPick,
) -> ComposerTargetPick {
    let picked = match pick {
        ComposerTargetPick::Live(employee) | ComposerTargetPick::Stale(employee) => employee,
        ComposerTargetPick::Unpicked => return pick,
    };
    match context {
        BossCommandContext::Employee(chat) if picked != *chat => ComposerTargetPick::Unpicked,
        _ => pick,
    }
}

/// Keep the accepted draft and its armed context together while asking the
/// owning daemon for the current chat. Pushes are only a cache hint here.
struct BossComposerSend {
    command: BossCommand,
    submission: ComposerSubmission,
}

impl BossCommand {
    fn resolve(
        &mut self,
        operation: BossOperation,
        mut request: impl FnMut(waku_client::Command) -> anyhow::Result<waku_client::ResponsePayload>,
    ) -> anyhow::Result<(AgentSession, Project)> {
        let waku_client::ResponsePayload::Boss {
            result: BossResult::Session { session, project },
        } = request(waku_client::Command::Boss { operation })?
        else {
            anyhow::bail!("invalid Boss chat response");
        };
        anyhow::ensure!(project.id == self.identity.id, "Boss identity changed");
        // Open returns a list projection. Hydrate before preparing a runtime
        // so the new chat keeps its permissions, model options and handoff.
        let waku_client::ResponsePayload::Session {
            session: Some(hydrated),
        } = request(waku_client::Command::HydrateSession {
            session_id: session.id,
        })?
        else {
            anyhow::bail!("the active Boss chat is unavailable");
        };
        anyhow::ensure!(
            hydrated.id == session.id
                && hydrated.project_id == project.id
                && hydrated.boss_managed
                && hydrated.planning.is_none()
                && hydrated.archived_at.is_none(),
            "the active Boss chat changed while opening"
        );
        self.session_id = hydrated.id;
        Ok((hydrated, *project))
    }
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
    default_purpose: Option<waku_protocol::boss::EmployeePurpose>,
    original_default_purpose: Option<waku_protocol::boss::EmployeePurpose>,
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
    /// A transcript memory card's Undo — the retracted note's identity
    /// rides `pending_memory_retracts` since this enum stays `Copy`.
    MemoryRetract,
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
    /// The paused-refresh banner's retry — confirms the dispatch and
    /// refreshes state like `Resume`.
    RotationRetry,
    /// A `/clear` dispatch — its Session reply takes the same landing the
    /// boss row's Open gets: swap bookkeeping, catalog insert, activation.
    Clear,
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

/// Active employees keep newest-summon-first order. Finished employees sort
/// by their most recent ended turn, falling back to their summon stamp for
/// sessions without a completion timestamp.
fn sort_boss_employees(
    employees: &mut Vec<&waku_protocol::boss::BossEmployee>,
    sessions: &HashMap<Uuid, &waku_protocol::model::AgentSession>,
) {
    employees.sort_by_key(|employee| {
        let session = sessions.get(&employee.session_id).copied();
        let timestamp = if employee.expired {
            session
                .and_then(|session| {
                    session
                        .turns
                        .iter()
                        .filter_map(|turn| turn.completed_at)
                        .max()
                })
                .or_else(|| session.and_then(|session| session.last_reply_at))
                .or(employee.created_at)
                .or_else(|| session.map(|session| session.created_at))
                .unwrap_or(0)
        } else {
            employee
                .created_at
                .or_else(|| session.map(|session| session.created_at))
                .unwrap_or(0)
        };
        (
            employee.expired,
            std::cmp::Reverse(timestamp),
            employee.session_id,
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

/// The boss document's `session_id` trails the task catalog across a
/// rotation: the retired chat is archived and its replacement pushed with
/// the task state while the document only lands on the next boss load — and
/// a submission parked on the retired chat keeps its id outright. Resolve a
/// recorded boss chat to the live one while the client can tell them apart:
/// the session itself while it still takes prompts, the swap the client
/// already recorded, or the unarchived boss-managed chat that shares the
/// retired chat's project — exactly one can be live, and planning sessions
/// are a different kind. Anything else passes through.
fn live_boss_chat_session(
    sessions: &[AgentSession],
    identities: &HashSet<Uuid>,
    rotated: &HashMap<Uuid, Uuid>,
    recorded: Uuid,
) -> Uuid {
    // A state refresh can land while the catalog still trails; follow
    // recorded swaps to their end so chained rotations resolve too.
    let mut resolved = recorded;
    for _ in 0..8 {
        match rotated.get(&resolved) {
            Some(&next) if next != resolved => resolved = next,
            _ => break,
        }
    }
    let Some(entry) = sessions.iter().find(|session| session.id == resolved) else {
        return resolved;
    };
    if entry.archived_at.is_none()
        || !entry.boss_managed
        || entry.planning.is_some()
        || !identities.contains(&entry.project_id)
    {
        return resolved;
    }
    let mut replacements = sessions.iter().filter(|session| {
        session.id != resolved
            && session.boss_managed
            && session.planning.is_none()
            && session.project_id == entry.project_id
            && session.archived_at.is_none()
    });
    match (replacements.next(), replacements.next()) {
        (Some(session), None) => session.id,
        _ => resolved,
    }
}

/// The session an activation or selection should land on: a retired Boss
/// chat's id resolves to its live replacement when the catalog already
/// carries it — the rotation's session swap is not a navigation boundary.
/// Anything else — a live chat, an unrelated session, a replacement the
/// push has not delivered yet — answers the requested id.
fn boss_chat_activation_target(
    sessions: &[AgentSession],
    identities: &HashSet<Uuid>,
    rotated: &HashMap<Uuid, Uuid>,
    requested: Uuid,
) -> Uuid {
    let resolved = live_boss_chat_session(sessions, identities, rotated, requested);
    if resolved != requested && sessions.iter().any(|session| session.id == resolved) {
        resolved
    } else {
        requested
    }
}

/// Whether the session is a Boss chat a rotation retired — an archived,
/// boss-managed, non-planning session inside a boss identity's workspace.
/// The document's pointer may still name it while the swap's pushes are in
/// flight, so it is never a surface to activate or restore.
fn is_retired_boss_chat(session: &AgentSession, identities: &HashSet<Uuid>) -> bool {
    session.archived_at.is_some()
        && session.boss_managed
        && session.planning.is_none()
        && identities.contains(&session.project_id)
}

/// The chat a boss-row `Open` may activate locally while another request
/// holds the pipe: the document's pointer resolved through the recorded
/// swaps and the catalog. `None` parks the request — a pointer naming a
/// retired chat, or a session the catalog has not received yet, is stale
/// state only the daemon's reply can heal; activating it would strand the
/// click on the archived predecessor or do nothing at all.
fn pending_boss_open_session<'a>(
    sessions: &'a [AgentSession],
    identities: &HashSet<Uuid>,
    rotated: &HashMap<Uuid, Uuid>,
    recorded: Option<Uuid>,
) -> Option<&'a AgentSession> {
    let resolved = live_boss_chat_session(sessions, identities, rotated, recorded?);
    sessions
        .iter()
        .find(|session| session.id == resolved)
        .filter(|session| !is_retired_boss_chat(session, identities))
}

/// Whether a parked boss `Open` still re-issues once the pipe frees — only
/// while the selection sits where the click left it. A selection that moved
/// to the recorded chat's live replacement is the rotation repointing
/// underneath the click, not the user leaving the surface.
fn queued_boss_open_still_applies(
    sessions: &[AgentSession],
    identities: &HashSet<Uuid>,
    rotated: &HashMap<Uuid, Uuid>,
    from: Option<Uuid>,
    selected: Option<Uuid>,
) -> bool {
    selected == from
        || from
            .zip(selected)
            .is_some_and(|(from, selected)| {
                live_boss_chat_session(sessions, identities, rotated, from) == selected
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
                self.migrate_boss_annotations(key, from, to, cx);
                self.migrate_voice_scratchpad(from, to);
                self.migrate_boss_composer_draft(from, to, cx);
                self.migrate_boss_queue_drain(from, to);
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
            self.boss_ui.employee_purposes.clear();
            self.boss_ui.expired.clear();
            self.boss_ui.attention.clear();
            self.boss_ui.workspace_transitions.clear();
            self.boss_ui.queued.clear();
            self.boss_ui.dispatching.clear();
            self.boss_ui.queued_model_targets.clear();
            let sessions_by_id: HashMap<Uuid, &waku_protocol::model::AgentSession> = self
                .state
                .sessions
                .iter()
                .map(|session| (session.id, session))
                .collect();
            for (key, state) in &self.boss_ui.states {
                if let Some(id) = state.session_id {
                    self.boss_ui.managed.insert(id);
                }
                let queue_rank = boss_queue_ranks(state);
                let mut employees = state.employees.iter().collect::<Vec<_>>();
                sort_boss_employees(&mut employees, &sessions_by_id);
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
                    if let Some(purpose) = employee.purpose {
                        self.boss_ui
                            .employee_purposes
                            .insert(employee.session_id, purpose);
                    }
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
                    if employee_attention_is_unread(employee) {
                        self.boss_ui.attention.insert(employee.session_id);
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
            // A ticket now carrying the force flag — or no longer queued at
            // all — has caught up with the press; the card reads the ticket.
            self.boss_ui.start_now_pending.retain(|id| {
                self.boss_ui
                    .states
                    .values()
                    .flat_map(|state| state.employees.iter())
                    .any(|employee| {
                        employee.session_id == *id
                            && employee.lifecycle()
                                == waku_protocol::boss::EmployeeLifecycle::Queued
                            && !employee
                                .ticket
                                .as_ref()
                                .is_some_and(|ticket| ticket.force_start)
                    })
            });
            self.boss_ui.revision = self.boss_ui.revision.wrapping_add(1);
            self.sync_boss_page_rows();
            // A boss chat opening or an employee expiring can arm or
            // disarm the composer's command target.
            self.sync_composer_placeholder(cx);
            // Catalog snapshots omit the Boss pseudo-project. Recover it
            // for a restored chat without activating or uncovering a page.
            // A refresh itself uses only catalog hydration below.
            if let Some(key) = self.boss_chat_key()
                && self
                    .boss_ui
                    .states
                    .get(&key)
                    .is_some_and(|state| !self.boss_ui.projects.contains_key(&state.identity.id))
            {
                let session = self.selected_session();
                let operation = BossOperation::Open {
                    provider: session.map_or(ProviderKind::Codex, |session| session.provider),
                    model: session.and_then(|session| session.model.clone()),
                    mode: session
                        .map(|session| session.runtime_mode)
                        .unwrap_or_default(),
                };
                self.boss_request(key, operation, BossReply::List, cx);
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
        if let Some(key) = finalized_plan_host {
            self.chat_with_boss(key, cx);
        }
        self.reconcile_boss_chat_refresh(cx);
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
        if self.boss_ui.page.map(|(_, open)| open) == Some(BossTab::Memory)
            && tab != BossTab::Memory
        {
            self.commit_memory_note_editor(cx);
        }
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
            // Passive pseudo-project recovery can wait for the next state
            // arrival; it must never use the click's activation shortcut.
            if reply == BossReply::List && matches!(operation, BossOperation::Open { .. }) {
                return;
            }
            // A boss-chat click while a request is in flight must not die —
            // under exactly the slowness an Open suffers, a dropped click
            // reads as a broken sidebar. A live session the app already
            // holds activates straight from its copy; anything else (first
            // open, a session the catalog lost, a pointer still naming the
            // chat a rotation retired) parks the operation so the
            // completion re-issues it against the daemon — only the
            // daemon's reply can heal a stale pointer. Last click wins.
            if let BossOperation::Open { .. } = operation {
                let identities: HashSet<Uuid> = self
                    .boss_ui
                    .states
                    .values()
                    .map(|state| state.identity.id)
                    .collect();
                // The document pointer trails the catalog across a
                // rotation — resolve it through the recorded swaps, and
                // treat a retired or missing target as stale state only a
                // real Open can heal.
                let session_id = pending_boss_open_session(
                    &self.state.sessions,
                    &identities,
                    &self.boss_ui.rotated_chat_drafts,
                    self.boss_ui
                        .states
                        .get(&key)
                        .and_then(|state| state.session_id),
                )
                .map(|session| session.id);
                if let Some(session_id) = session_id {
                    self.request_session_activation(
                        session_id,
                        SessionActivationTransition::Visit,
                        cx,
                    );
                } else {
                    self.boss_ui.queued_open =
                        Some((key, operation, self.state.selected_session));
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
            } else if matches!(
                &operation,
                BossOperation::Memory {
                    operation: MemoryOperation::Retract { .. }
                }
            ) && reply == BossReply::MemoryRetract
            {
                // An undo behind another request parks in click order —
                // every retract must reach the daemon.
                self.boss_ui
                    .queued_memory_retract
                    .push_back((key, operation));
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
        // An Open's reply navigates only while the surface it was issued
        // from survives — a picker commit or a sidebar click in between
        // means the user moved on and the late reply must not drag them
        // back. A `/clear` follows the same contract: the fresh chat opens
        // only while the composer still shows the chat it replaced.
        self.boss_ui.open_from =
            if matches!(operation, BossOperation::Open { .. } | BossOperation::Clear) {
                self.state.selected_session
            } else {
                None
            };
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
                            BossResult::PlanRejected { error } => {
                                this.show_toast(tr!("boss.failed", error = error.detail));
                                if let BossReply::Finalize(session_id) = reply {
                                    this.plan_docs.remove(&session_id);
                                    if let Some(plan_file) = this
                                        .state
                                        .sessions
                                        .iter()
                                        .find(|session| session.id == session_id)
                                        .and_then(|session| session.planning.as_ref())
                                        .map(|planning| planning.plan_file.clone())
                                    {
                                        this.ensure_plan_doc(key, session_id, &plan_file, true, cx);
                                    }
                                }
                            }
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
                                if matches!(reply, BossReply::Open | BossReply::Clear | BossReply::List) =>
                            {
                                this.daemons.claim_project(project.id, key);
                                this.boss_ui.projects.insert(project.id, *project);
                                // An Open answered before the pointer commit
                                // can carry the retired chat itself — only a
                                // live session is a swap destination, so the
                                // reply never regresses the pointer or records
                                // a backwards new→old swap.
                                let rotated_from = if session.planning.is_none()
                                    && session.archived_at.is_none()
                                    && this.resolve_boss_chat_session(session.id) == session.id
                                {
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
                                    this.migrate_boss_queue_drain(from, id);
                                }
                                this.daemons.claim_session(id, key);
                                if session.detail_loaded {
                                    if let Some(existing) =
                                        this.state.sessions.iter_mut().find(|entry| entry.id == id)
                                    {
                                        *existing = *session;
                                    } else {
                                        this.state.sessions.push(*session);
                                    }
                                } else {
                                    // Open returns a list projection for an
                                    // existing chat. Preserve resident details
                                    // and rotation links, just as catalog sync does.
                                    let runtime_ids: HashSet<_> =
                                        this.runtimes.keys().copied().collect();
                                    super::runtime::merge_remote_session_catalog(
                                        &mut this.state.sessions,
                                        vec![*session],
                                        |_| false,
                                        |id| runtime_ids.contains(&id),
                                        false,
                                    );
                                }
                                let navigates = matches!(reply, BossReply::Open | BossReply::Clear)
                                    && boss_open_reply_navigates(
                                        this.boss_ui.open_from,
                                        this.state.selected_session,
                                    );
                                this.reconcile_boss_chat_refresh(cx);
                                if navigates {
                                    this.request_session_activation(
                                        id,
                                        SessionActivationTransition::Visit,
                                        cx,
                                    );
                                }
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
                            BossResult::Memory { retracted, .. }
                                if reply == BossReply::MemoryRetract =>
                            {
                                let pending = this.boss_ui.pending_memory_retracts.pop_front();
                                if retracted.is_some()
                                    && let Some((retract_key, bucket, note_id)) = pending
                                {
                                    this.boss_ui.retracted_memory.insert((
                                        retract_key,
                                        bucket.clone(),
                                        note_id.clone(),
                                    ));
                                    if let Some(feed) =
                                        this.boss_ui.memory_feed.get_mut(&retract_key)
                                    {
                                        feed.records.retain(|record| {
                                            !(record.bucket_id == bucket && record.id == note_id)
                                        });
                                        feed.pending_refresh.retain(|record| {
                                            !(record.bucket_id == bucket && record.id == note_id)
                                        });
                                    }
                                    this.refresh_memory_note_status();
                                    this.show_success_toast(tr!("boss.memory_removed_toast"));
                                }
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
                        if matches!(reply, BossReply::RotationRetry) {
                            this.show_success_toast(tr!("boss.rotation_retry_sent"));
                            this.refresh_boss_state_on(key, cx);
                        }
                        if matches!(reply, BossReply::Clear) {
                            this.show_success_toast(tr!("boss.cleared"));
                            // The Session arm above already landed the swap;
                            // this pull keeps the roster and pointers honest
                            // while the daemon's pushes are still in flight.
                            this.refresh_boss_state_on(key, cx);
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
                        } else if reply == BossReply::MemoryRetract {
                            let pending = this.boss_ui.pending_memory_retracts.pop_front();
                            // "Already retracted" and "unknown note" both
                            // leave the bucket without the record — the
                            // undo's end state — so the card settles rather
                            // than nagging about a race it already won.
                            if let Some((retract_key, bucket, note_id)) = pending {
                                if error.contains("retracted")
                                    || error.contains("unknown memory note")
                                {
                                    this.boss_ui.retracted_memory.insert((
                                        retract_key,
                                        bucket,
                                        note_id,
                                    ));
                                } else {
                                    this.show_toast(tr!("boss.failed", error = error.clone()));
                                }
                            } else {
                                this.show_toast(tr!("boss.failed", error = error.clone()));
                            }
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
                    && let Some((queued_key, operation, from)) =
                        this.boss_ui.queued_open.take()
                {
                    // The click that parked this open was issued on a surface
                    // the user has since left — re-issuing would yank them
                    // back to a chat they navigated away from. A selection
                    // that moved to the recorded chat's live replacement is
                    // the rotation repointing underneath, not a departure.
                    let identities: HashSet<Uuid> = this
                        .boss_ui
                        .states
                        .values()
                        .map(|state| state.identity.id)
                        .collect();
                    if queued_boss_open_still_applies(
                        &this.state.sessions,
                        &identities,
                        &this.boss_ui.rotated_chat_drafts,
                        from,
                        this.state.selected_session,
                    ) {
                        this.boss_request(queued_key, operation, BossReply::Open, cx);
                    }
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
                if !this.boss_ui.pending
                    && let Some((queued_key, operation)) =
                        this.boss_ui.queued_memory_retract.pop_front()
                {
                    this.boss_request(queued_key, operation, BossReply::MemoryRetract, cx);
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

    /// Whether a transcript memory card's note is gone from its bucket —
    /// the daemon-settled marker, a retract confirmed this run, or a
    /// loaded feed that no longer lists the record. A parked feed refresh
    /// still counts as present: additions waiting behind "New memories
    /// available" are not proof of removal.
    pub(super) fn memory_card_removed(
        &self,
        key: DaemonKey,
        card: &waku_protocol::model::MemoryRecordCard,
    ) -> bool {
        if self.boss_ui.retracted_memory.contains(&(
            key,
            card.bucket_id.clone(),
            card.note_id.clone(),
        )) {
            return true;
        }
        let Some(feed) = self.boss_ui.memory_feed.get(&key) else {
            return false;
        };
        // A feed fetched before the note existed cannot testify either
        // way — only a fetch at or after its record time can call it gone.
        feed.loaded
            && feed
                .refreshed_at
                .is_some_and(|refreshed| refreshed >= card.created_at)
            && !feed
                .records
                .iter()
                .chain(feed.pending_refresh.iter())
                .any(|record| record.bucket_id == card.bucket_id && record.id == card.note_id)
    }

    /// The memory card's Undo: retract the note from its bucket and settle
    /// the card in this transcript — `session_id` names the transcript so
    /// the daemon patches the persisted marker in place.
    pub(super) fn retract_memory_record(
        &mut self,
        session_id: Uuid,
        card: waku_protocol::model::MemoryRecordCard,
        cx: &mut Context<Self>,
    ) {
        let key = self.daemons.session_owner(session_id);
        self.boss_ui.pending_memory_retracts.push_back((
            key,
            card.bucket_id.clone(),
            card.note_id.clone(),
        ));
        self.boss_request(
            key,
            BossOperation::Memory {
                operation: MemoryOperation::Retract {
                    bucket: Some(card.bucket_id.clone()),
                    project: None,
                    note_id: card.note_id.clone(),
                    session: Some(session_id),
                },
            },
            BossReply::MemoryRetract,
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
            feed.refreshed_at = Some(unix_time());
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
        // A record that left the corpus — deleted, or no longer accessible
        // — takes its pending notes with it.
        self.prune_memory_feedback(key, cx);
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
        self.prune_memory_feedback(key, cx);
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
        let identity = self
            .selected_session()
            .filter(|session| session.boss_managed && session.planning.is_none())
            .map(|session| session.project_id);
        self.boss_ui.states.iter().find_map(|(key, state)| {
            // Keep the mounted predecessor a Boss chat while the catalog
            // and successor hydration catch up with the new pointer.
            (state.session_id == Some(id) || identity == Some(state.identity.id)).then_some(*key)
        })
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

    /// An armed command or a parked submission can outlive the boss chat it
    /// names: rotation archives the session and publishes its replacement
    /// while the boss document's pointer is still in flight, and the daemon
    /// refuses sends to the retired chat. Retarget retired boss chats to
    /// the live replacement so the send reaches the active session; every
    /// other id passes through.
    pub(super) fn resolve_boss_chat_session(&self, session_id: Uuid) -> Uuid {
        let identities: HashSet<Uuid> = self
            .boss_ui
            .states
            .values()
            .map(|state| state.identity.id)
            .collect();
        live_boss_chat_session(
            &self.state.sessions,
            &identities,
            &self.boss_ui.rotated_chat_drafts,
            session_id,
        )
    }

    /// `resolve_boss_chat_session` narrowed to destinations the catalog can
    /// land on — the live replacement's id when a retired chat's successor
    /// is already present, `None` for a live chat, an unrelated session, or
    /// a replacement whose row has not been pushed yet.
    pub(super) fn live_boss_chat_target(&self, session_id: Uuid) -> Option<Uuid> {
        let identities: HashSet<Uuid> = self
            .boss_ui
            .states
            .values()
            .map(|state| state.identity.id)
            .collect();
        let resolved = boss_chat_activation_target(
            &self.state.sessions,
            &identities,
            &self.boss_ui.rotated_chat_drafts,
            session_id,
        );
        (resolved != session_id).then_some(resolved)
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
        // Pending memory notes reroute the next submission at their boss —
        // live while the Memory page (or the boss chat itself) is on screen.
        if let Some(key) = self.memory_feedback_armed_daemon()
            && let Some(state) = self.boss_ui.states.get(&key)
            && let Some(session_id) = state.session_id
        {
            return Some(BossCommand {
                session_id: self.resolve_boss_chat_session(session_id),
                identity: state.identity.clone(),
                context: BossCommandContext::MemoryFeedback(key),
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
                            session_id: self.resolve_boss_chat_session(state.session_id?),
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
            session_id: self.resolve_boss_chat_session(state.session_id?),
            identity: state.identity.clone(),
            context: BossCommandContext::Employee(employee_id),
        })
    }

    /// The daemon whose boss owns an armed command — the chat submissions
    /// land on and the roster the target chip lists.
    pub(super) fn composer_command_daemon(&self, command: &BossCommand) -> Option<DaemonKey> {
        self.boss_ui
            .states
            .iter()
            .find_map(|(key, state)| (state.identity.id == command.identity.id).then_some(*key))
    }

    /// The chip pick for an armed command's daemon: `Boss` until the user
    /// picks an employee off the menu. A pick names a session id, not a
    /// row, so roster reorders cannot retarget it; one whose employee
    /// left the roster comes back `Stale` so sends refuse closed instead
    /// of landing on the boss chat.
    pub(super) fn composer_target(&self, command: &BossCommand) -> ComposerTarget {
        let Some(key) = self.composer_command_daemon(command) else {
            return ComposerTarget::Boss;
        };
        match composer_target_pick_for_context(
            &command.context,
            self.boss_ui.composer_target_pick(key),
        ) {
            ComposerTargetPick::Unpicked => ComposerTarget::Boss,
            ComposerTargetPick::Live(employee) => ComposerTarget::Employee(employee),
            ComposerTargetPick::Stale(employee) => ComposerTarget::Stale {
                employee,
                name: self.employee_target_name(employee),
            },
        }
    }

    /// The label a picked employee wears on the chip and in refusal
    /// toasts — its persona name, falling back to the session title and
    /// then a bare "Employee" once even the roster record is gone.
    fn employee_target_name(&self, employee: Uuid) -> String {
        self.boss_ui
            .identities
            .get(&employee)
            .map(|identity| identity.name.clone())
            .or_else(|| {
                self.state
                    .sessions
                    .iter()
                    .find(|session| session.id == employee)
                    .map(|session| session.title.clone())
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| tr!("boss.trigger_employee"))
    }

    /// The identity a picked employee wears on the chip and in the send-to
    /// menu — its persona identity while the roster knows it, a
    /// name-derived stand-in once it does not, so the row never loses its
    /// face.
    fn employee_target_identity(&self, employee: Uuid) -> BossIdentity {
        self.boss_ui
            .identities
            .get(&employee)
            .cloned()
            .unwrap_or_else(|| {
                let name = self.employee_target_name(employee);
                BossIdentity {
                    id: employee,
                    name: name.clone(),
                    avatar_seed: name,
                    avatar_style: AvatarStyle::default(),
                }
            })
    }

    /// Record the destination chip's pick for one boss daemon — `None`
    /// restores the boss chat as the landing.
    fn set_composer_target(
        &mut self,
        key: DaemonKey,
        target: Option<Uuid>,
        cx: &mut Context<Self>,
    ) {
        if self.boss_ui.composer_targets.get(&key).copied() == target {
            return;
        }
        let interaction = self.composer_boss_command().and_then(|command| {
            let was_boss = matches!(self.composer_target(&command), ComposerTarget::Boss);
            employee_composer_interaction(
                &command.context, self.state.selected_session, target, was_boss,
            ).then_some(target).flatten()
        });
        self.boss_ui.employee_composer_interactions.remove(&key);
        if let Some(employee) = interaction {
            self.boss_ui
                .employee_composer_interactions
                .insert(key, employee);
        }
        match target {
            Some(employee) => {
                self.boss_ui.composer_targets.insert(key, employee);
            }
            None => {
                self.boss_ui.composer_targets.remove(&key);
            }
        }
        self.sync_composer_placeholder(cx);
        cx.notify();
    }

    /// The attachment a boss-command submission carries: a session
    /// reference for the viewed employee, the published file itself for
    /// an armed deliverable. Its mention token splices into the provider-
    /// facing prompt while the chip lands in the boss transcript's bubble.
    /// Memory feedback carries none — its notes ride the submission's
    /// annotations instead.
    fn boss_command_attachment(
        &self,
        context: &BossCommandContext,
    ) -> Option<MessageAttachment> {
        Some(match context {
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
            // Drained ahead of the attachment read — unreachable here.
            BossCommandContext::MemoryFeedback(_) => return None,
        })
    }

    /// Resolve an armed send on its owning daemon before any runtime-scoped
    /// send or queue decision. Neither Boss nor catalog pushes need to arrive.
    pub(super) fn submit_boss_composer_command(
        &mut self,
        command: BossCommand,
        submission: ComposerSubmission,
        steer: bool,
        cx: &mut Context<Self>,
    ) {
        let restore_session = self.state.selected_session.unwrap_or(command.session_id);
        let Some(key) = self.composer_command_daemon(&command) else {
            self.restore_composer_submission(restore_session, submission, cx);
            return;
        };
        let Some(client) = self.daemons.supervisor(key).map(|daemon| daemon.client()) else {
            self.restore_composer_submission(restore_session, submission, cx);
            self.show_toast(tr!("boss.unreachable"));
            return;
        };
        let cached = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == command.session_id);
        let operation = BossOperation::Open {
            provider: cached
                .map(|session| session.provider)
                .unwrap_or(ProviderKind::Codex),
            model: cached.and_then(|session| session.model.clone()),
            mode: cached
                .map(|session| session.runtime_mode)
                .unwrap_or_default(),
        };
        let queue_background = self.live_deliverable_page().is_some();
        let old = command.session_id;
        let mut command = command;
        cx.spawn(async move |this, cx| {
            let (command, result) = cx
                .background_executor()
                .spawn(async move {
                    let result = command.resolve(operation, |command| {
                        client.request(Uuid::nil(), Uuid::nil(), command)
                    });
                    (command, result)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let send = BossComposerSend {
                    command,
                    submission,
                };
                let (session, project) = match result {
                    Ok(opened) => opened,
                    Err(error) => {
                        // No context was drained or disarmed yet: restore the
                        // original draft on its employee/deliverable surface.
                        this.restore_composer_submission(restore_session, send.submission, cx);
                        this.show_toast(tr!("boss.failed", error = error));
                        cx.notify();
                        return;
                    }
                };
                let id = session.id;
                this.daemons.claim_project(project.id, key);
                this.boss_ui.projects.insert(project.id, project);
                this.daemons.claim_session(id, key);
                if let Some(existing) = this.state.sessions.iter_mut().find(|entry| entry.id == id)
                {
                    // Keep newer live events if an attach beat this snapshot.
                    if !existing.detail_loaded {
                        *existing = session;
                    }
                } else {
                    this.state.sessions.push(session);
                }
                if old != id {
                    this.migrate_voice_scratchpad(old, id);
                    this.migrate_boss_composer_draft(old, id, cx);
                    this.migrate_boss_queue_drain(old, id);
                }
                if let Some(state) = this.boss_ui.states.get_mut(&key)
                    && state.session_id == Some(old)
                {
                    state.session_id = Some(id);
                }
                let (id, submission) =
                    this.boss_command_submission_parts(send.command, send.submission, cx);
                if steer && !queue_background {
                    this.steer_session_submission(id, submission, cx);
                } else {
                    this.submit_composer_submission_to_with_canned(
                        id,
                        submission,
                        None,
                        queue_background,
                        cx,
                    );
                }
            });
        })
        .detach();
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
        if let BossCommandContext::MemoryFeedback(daemon) = command.context {
            // Pending notes fold into the submission as numbered
            // annotations — merged with whatever the draft already staged
            // and kept in global creation order, so "Annotation N" in the
            // prompt, the bubble, and any reply citations all agree. A
            // failed send restores the drained batch through the ordinary
            // submission-restore path.
            let drained = self.drain_memory_feedback(daemon, cx);
            if !drained.is_empty() {
                let body = submission
                    .prompt
                    .strip_prefix(&annotation_prompt_prefix(&submission.annotations))
                    .unwrap_or(submission.prompt.as_str())
                    .trim_end()
                    .to_owned();
                submission.annotations.extend(drained);
                submission
                    .annotations
                    .sort_by_key(|annotation| annotation.id);
                // Feedback reads as a request — the Boss decides how to
                // reconcile its memory; nothing claims a record changed.
                let instruction = "Please consider this memory feedback and update the Boss-managed memory if appropriate.";
                submission.prompt = format!(
                    "{}{instruction}\n\n{body}",
                    annotation_prompt_prefix(&submission.annotations)
                )
                .trim_end()
                .to_owned();
                submission.display_content = Some(annotation_bubble_content(
                    &submission.annotations,
                    &body,
                ));
                submission.human_content = if body.is_empty() {
                    Some(annotation_display_content(&submission.annotations))
                } else {
                    None
                };
                self.show_toast(tr!("memory.feedback_sent"));
                self.note_user_message_target(command.session_id);
                // Re-activating the already-viewed boss chat resets its
                // transcript before the send path can honor the reader's
                // scroll preference.
                if self.state.selected_session != Some(command.session_id) {
                    self.request_session_activation(
                        command.session_id,
                        SessionActivationTransition::Visit,
                        cx,
                    );
                }
                self.sync_composer_placeholder(cx);
            }
            return (command.session_id, submission);
        }
        let Some(attachment) = self.boss_command_attachment(&command.context) else {
            return (command.session_id, submission);
        };
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
        submission.prompt = if prompt.is_empty() {
            token
        } else {
            format!("{prompt} {token}")
        };
        submission.attachments.push(attachment);
        self.boss_ui.command_deliverable = None;
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
        let placeholder = match self.composer_boss_command() {
            Some(command) => match self.composer_target(&command) {
                ComposerTarget::Employee(employee) => tr!(
                    "boss.command_placeholder_direct",
                    name = self.employee_target_name(employee)
                ),
                // The chip keeps naming the picked employee — the send
                // itself refuses, so the hint stays honest about intent.
                ComposerTarget::Stale { name, .. } => {
                    tr!("boss.command_placeholder_direct", name = name)
                }
                ComposerTarget::Boss => match command.context {
                    BossCommandContext::Employee(..) => {
                        tr!("boss.command_placeholder_employee")
                    }
                    BossCommandContext::Deliverable { .. } => {
                        tr!("boss.command_placeholder_deliverable")
                    }
                    BossCommandContext::MemoryFeedback(_) => {
                        tr!("boss.command_placeholder_memory_feedback")
                    }
                },
            },
            None => tr!("input.do_anything"),
        };
        self.composer
            .update(cx, |composer, cx| composer.set_placeholder(placeholder, cx));
    }

    /// The composer's destination chip while a boss command is armed: the
    /// face and name Enter's draft lands on — the boss by default, or the
    /// employee picked off this menu, which sends over the roster's
    /// control channel instead of commanding the boss. A stale pick keeps
    /// its employee's name so the chip never claims a destination a send
    /// would silently miss.
    pub(super) fn render_boss_command_chip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let command = self.composer_boss_command()?;
        // The workspace footer already names the boss when the command's
        // own chat is the viewed session — one avatar and name, not two.
        if self.workspace_subject().0 == Some(command.session_id) {
            return None;
        }
        let theme = Theme::current(cx);
        let key = self.composer_command_daemon(&command)?;
        let target = self.composer_target(&command);
        let (chip_name, chip_identity) = match &target {
            ComposerTarget::Boss => (
                command.identity.name.clone(),
                Some(command.identity.clone()),
            ),
            ComposerTarget::Employee(employee) | ComposerTarget::Stale { employee, .. } => {
                let name = self.employee_target_name(*employee);
                (name, Some(self.employee_target_identity(*employee)))
            }
        };
        // Menu rows are a prebuilt snapshot: the items closure and its row
        // renders run inside the card's own paint, where the entity is
        // already borrowed, so faces come from the avatar cache only.
        let picked = self.boss_ui.composer_targets.get(&key).copied();
        // An employee chat's send-to menu holds only its own employee
        // beside the boss — colleagues stay off the chat's composer
        // entirely. The row does not need roster membership: a retired
        // employee's page still offers it, and the pick reads stale on
        // send. Other armed contexts keep the whole roster.
        let chat_employee = match &command.context {
            BossCommandContext::Employee(employee) => Some(*employee),
            _ => None,
        };
        // The check lands on the destination a send would take: the
        // chat's own employee when picked, the boss for anything else an
        // employee page cannot honor.
        let boss_selected = match chat_employee {
            Some(employee) => picked != Some(employee),
            None => picked.is_none(),
        };
        let boss_face = self.boss_avatar_cached(&command.identity, 18.0);
        let boss_identity = command.identity.clone();
        let boss_detail = chat_employee.map(|_| tr!("boss.command_target_boss"));
        let employees: Vec<(Uuid, BossIdentity, Option<String>)> = match chat_employee {
            Some(employee) => vec![(
                employee,
                self.employee_target_identity(employee),
                Some(tr!("boss.command_target_employee")),
            )],
            None => self
                .boss_ui
                .recent
                .get(&key)
                .into_iter()
                .flatten()
                .filter_map(|employee| {
                    let identity = self.boss_ui.identities.get(employee)?.clone();
                    let job_title = self
                        .boss_ui
                        .job_titles
                        .get(employee)
                        .cloned()
                        .unwrap_or_default();
                    let detail = match (
                        job_title.is_empty(),
                        self.boss_ui.expired.contains(employee),
                    ) {
                        (false, true) => {
                            format!("{} · {}", job_title, tr!("boss.employee_finished"))
                        }
                        (true, true) => tr!("boss.employee_finished"),
                        _ => job_title,
                    };
                    Some((
                        *employee,
                        identity,
                        (!detail.is_empty()).then_some(detail),
                    ))
                })
                .collect(),
        };
        let employee_faces: Vec<Option<Arc<gpui::RenderImage>>> = employees
            .iter()
            .map(|(_, identity, _)| self.boss_avatar_cached(identity, 18.0))
            .collect();
        let weak = cx.entity().downgrade();
        let handle = self.menu_handle("boss-composer-target", cx);
        let trigger = div()
            .id("boss-command-chip")
            .flex_none()
            .h(px(26.0))
            .pl(px(5.0))
            .pr(px(7.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(sp(13.0))
            .line_height(sp(16.0))
            .cursor_default()
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .when(handle.is_open(), |element| element.bg(theme.overlay))
            .hover(|element| element.bg(theme.overlay))
            .tooltip(Tooltip::text(tr!(
                "boss.command_hint",
                name = chip_name.clone()
            )))
            .when_some(chip_identity, |element, identity| {
                element.child(self.boss_avatar(&identity, 16.0, cx))
            })
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.text_secondary)
                    .child(SharedString::from(chip_name)),
            )
            .child(icon(
                "icons/chevron-down.svg",
                10.5,
                theme.affordance_icon(),
            ));
        Some(dropdown_menu(
            trigger,
            "boss-composer-target-menu",
            &handle,
            MenuAlign::AboveLeft,
            move |cx| {
                let mut items = Vec::new();
                items.push(MenuItem::Header(tr!("boss.command_send_to").into()));
                {
                    let weak = weak.clone();
                    items.push(
                        MenuItem::custom({
                            let theme = Theme::current(cx);
                            let boss_face = boss_face.clone();
                            let identity = boss_identity.clone();
                            let name = boss_identity.name.clone();
                            let detail = boss_detail.clone();
                            move |_, _| {
                                target_row(
                                    boss_face.clone(),
                                    &identity,
                                    &name,
                                    detail.clone(),
                                    boss_selected,
                                    &theme,
                                )
                            }
                        })
                        .on_click(move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.set_composer_target(key, None, cx);
                            });
                        }),
                    );
                }
                if employees.is_empty() {
                    items.push(
                        MenuItem::new(tr!("boss.command_no_employees"), move |_, _| {})
                            .disabled(true),
                    );
                } else {
                    items.push(MenuItem::Separator);
                    for (index, (employee, identity, detail)) in employees.iter().enumerate() {
                        let weak = weak.clone();
                        let employee = *employee;
                        items.push(
                            MenuItem::custom({
                                let theme = Theme::current(cx);
                                let face = employee_faces[index].clone();
                                let identity = identity.clone();
                                let name = identity.name.clone();
                                let detail = detail.clone();
                                let selected = picked == Some(employee);
                                move |_, _| {
                                    target_row(
                                        face.clone(),
                                        &identity,
                                        &name,
                                        detail.clone(),
                                        selected,
                                        &theme,
                                    )
                                }
                            })
                            .on_click(move |_, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.set_composer_target(key, Some(employee), cx);
                                });
                            }),
                        );
                    }
                }
                items
            },
        ))
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

    /// Whether the session is a Boss chat a rotation retired — an archived
    /// boss-managed session in a boss's own workspace, whatever the
    /// document's pointer still says. Distinct from an archived employee:
    /// the predecessor is history inside the live chat, never a surface to
    /// restore.
    pub(super) fn session_is_retired_boss_chat(&self, session: &AgentSession) -> bool {
        is_retired_boss_chat(
            session,
            &self
                .boss_ui
                .states
                .values()
                .map(|state| state.identity.id)
                .collect(),
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
        if avatar_style == AvatarStyle::Mascots {
            boss_moods::mascots::prefetch();
        }
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
                        self.employee_plan_recovery_controls(key, employee, "header", &theme, cx)
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
            || editor.default_purpose != editor.original_default_purpose
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
        // A shipped default's editor owns only the appended
        // customization — the shipped base stays read-only in the
        // detail pane and reattaches on save, so an edit can never
        // fork or lose the default itself.
        let canonical_role = persona.as_ref().and_then(|entry| {
            self.boss_ui
                .states
                .get(&key)
                .and_then(|state| state.persona_default_role(entry.id))
        });
        let content = persona
            .as_ref()
            .map(|entry| match canonical_role {
                Some(role) => shipped_persona_prefix(role, &entry.markdown)
                    .map(|(_, customization)| customization.to_owned())
                    .unwrap_or_else(|| entry.markdown.clone()),
                None => entry.markdown.clone(),
            })
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
            default_purpose: persona.as_ref().and_then(|entry| entry.default_purpose),
            original_default_purpose: persona.as_ref().and_then(|entry| entry.default_purpose),
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
                    default_purpose: Some(editor.default_purpose),
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
        // Warm the pack while the queued render job waits on it — Once +
        // thread spawn only, no I/O on this path.
        if identity.avatar_style == AvatarStyle::Mascots {
            boss_moods::mascots::prefetch();
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

    /// `boss_avatar_animated`'s choice resolved to prepared rasters — the
    /// animated Gaze layers when the style animates and they are already
    /// rendered, else the still or its placeholder. Callers fill the
    /// shared row's avatar field while they still hold the view.
    pub(super) fn prepare_boss_employee_avatar(
        &self,
        identity: &BossIdentity,
        size: f32,
        cx: &App,
    ) -> BossEmployeeAvatar {
        if gaze_animates(identity.avatar_style, cx.reduce_motion())
            && let Some(faces) = self.gaze_faces(identity, size)
        {
            return BossEmployeeAvatar::Gaze {
                faces,
                shift: gaze_eye_shift(&identity.avatar_seed),
            };
        }
        BossEmployeeAvatar::Still {
            image: self.boss_avatar_image(identity, size),
            circular: boss_moods::circular_frame(identity.avatar_style),
            initial: SharedString::from(identity.name.chars().next().unwrap_or('B').to_string()),
        }
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
                        // Mascots are raster files from the cloned pack, not
                        // generated SVG — decode straight to a RenderImage.
                        if style == AvatarStyle::Mascots {
                            let still =
                                boss_moods::mascots::avatar_image(&avatar_seed, bucket)?;
                            return Ok::<_, anyhow::Error>(AvatarFaces {
                                still,
                                gaze: None,
                            });
                        }
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

    /// The selected session's queued-employee record and the daemon that
    /// owns it. The tail card exists exactly while this resolves — a ticket
    /// leaving `Queued` (dispatch, stop, expiry) drops the row, and the
    /// fold's fade retention handles the exit.
    pub(super) fn selected_queued_employee(
        &self,
    ) -> Option<(DaemonKey, &waku_protocol::boss::BossEmployee)> {
        let session_id = self.state.selected_session?;
        self.boss_ui.states.iter().find_map(|(key, state)| {
            state
                .employees
                .iter()
                .find(|employee| {
                    employee.session_id == session_id
                        && employee.lifecycle() == waku_protocol::boss::EmployeeLifecycle::Queued
                })
                .map(|employee| (*key, employee))
        })
    }

    /// The queued card's display model for the selected session — `None`
    /// once the ticket is past the queue (the fading ghost renders from
    /// `QueuedCardFade` instead).
    pub(super) fn queued_employee_card(&self) -> Option<QueuedEmployeeCard> {
        let (key, employee) = self.selected_queued_employee()?;
        let state = self.boss_ui.states.get(&key)?;
        let position = boss_queue_position(state, employee.session_id);
        let (phase, headline, secondary) = queued_card_copy(
            employee,
            position,
            self.boss_ui
                .start_now_pending
                .contains(&employee.session_id),
        );
        Some(QueuedEmployeeCard {
            key,
            session_id: employee.session_id,
            phase,
            headline,
            secondary,
        })
    }

    /// The card's direct "Start now" — the human's `startNow` control op,
    /// not a prompt: the daemon commits the force flag on the ticket and
    /// the summon scheduler stays the single dispatch winner. The press is
    /// committed locally at once so the button settles into `Starting`
    /// before the ticket's flag or the reply lands; a request failure
    /// restores the waiting card with the error surfaced.
    pub(super) fn start_queued_employee_now(
        &mut self,
        key: DaemonKey,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.daemons.supervisor(key).map(|daemon| daemon.client()) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            return;
        };
        self.boss_ui.start_now_pending.insert(session_id);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::Boss {
                            operation: BossOperation::Control {
                                session_id,
                                action: waku_client::boss::EmployeeControl::StartNow,
                            },
                        },
                    )
                })
                .await;
            let _ = this.update(cx, move |this, cx| {
                this.boss_ui.start_now_pending.remove(&session_id);
                match result {
                    Ok(waku_client::ResponsePayload::Boss {
                        result:
                            BossResult::Summoned {
                                state, admission, ..
                            },
                    }) => {
                        // Apply the settled admission now rather than waiting
                        // on the next Boss snapshot — a still-blocked ticket
                        // reads declined immediately and a dispatching one
                        // drops the card without broadcast lag.
                        if let Some(employee) = this
                            .boss_ui
                            .states
                            .values_mut()
                            .flat_map(|boss_state| boss_state.employees.iter_mut())
                            .find(|employee| employee.session_id == session_id)
                        {
                            employee.state = state;
                            if let (Some(ticket), Some(admission)) =
                                (&mut employee.ticket, admission)
                            {
                                ticket.force_start = admission.force_start;
                                ticket.blocked_by = admission.blocked_by;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        this.show_toast(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
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
        let attention_unread = self.boss_ui.attention.contains(&id);
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
        let revealed_project = alt_held
            .then(|| {
                session
                    .and_then(|session| {
                        self.state
                            .projects
                            .iter()
                            .find(|project| project.id == session.project_id)
                            .filter(|project| !project.is_projectless())
                            .map(|project| project.display_name())
                    })
            })
            .flatten();
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
        // An expired row's archive control borrows the status slot the way
        // a task row's does — zero-width until the row is hovered or the
        // button takes keyboard focus. Archiving drops only the sidebar
        // listing; the brain page's employee history keeps the record.
        let expired = self.boss_ui.expired.contains(&id);
        let group_name = SharedString::from(format!("boss-employee-row-{id}"));
        let archive_button = expired.then(|| {
            let focus = self
                .sidebar_employee_archive_focuses
                .borrow_mut()
                .entry(id)
                .or_insert_with(|| cx.focus_handle())
                .clone();
            self.sidebar_deliverable_button(
                &group_name,
                format!("boss-employee-archive-{id}"),
                &focus,
                "icons/archive.svg",
                tr!("session.archive"),
                cx,
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.archive_sidebar_employee(id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.archive_sidebar_employee(id, cx);
                    cx.stop_propagation();
                }
            }))
        });
        div()
            .id(format!("boss-employee-{id}"))
            .when(expired, |row| row.group(group_name.clone()))
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
                revealed_project,
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
            // The flagged employee wears the same unread dot its folded
            // group header does — boss-flagged attention is the only
            // "unread" an employee row carries.
            .when(attention_unread, |row| {
                row.child(
                    div()
                        .id(format!("boss-employee-attention-{id}"))
                        .flex_none()
                        .size(px(12.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .tooltip(Tooltip::text(tr!("boss.employee_needs_attention")))
                        .child(div().size(px(7.0)).rounded_full().bg(theme.info)),
                )
            })
            .when_some(status_indicator, |row, indicator| {
                // On an expired row the status yields its slot to the
                // hover-revealed archive control — it collapses out of the
                // flow rather than just hiding so the button lands where
                // the indicator stood.
                if expired {
                    row.child(
                        div()
                            .flex_none()
                            .overflow_hidden()
                            .group_hover(group_name.clone(), |style| style.w_0().opacity(0.0))
                            .child(indicator),
                    )
                } else {
                    row.child(indicator)
                }
            })
            .when_some(archive_button, |row, button| row.child(button))
            .into_any_element()
    }

    /// An expired employee row's archive control: drops the row from the
    /// sidebar listing. The dismissal is sidebar-only and app-persisted —
    /// the daemon's roster is untouched, so the brain page's employee
    /// history still lists the record and reviving the employee resurfaces
    /// the row.
    pub(super) fn archive_sidebar_employee(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if !self.state.sidebar_archived_employees.insert(session_id) {
            return;
        }
        self.sidebar_rows_fingerprint.set(None);
        self.save();
        cx.notify();
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
        // The session's project carries the row's detail line, labeled the
        // way an ordinary task row names its project: the projectless
        // workspace reads "Chat", and a project missing from the catalog
        // owns up to being unknown rather than bluffing a name.
        let project = self
            .state
            .projects
            .iter()
            .find(|project| project.id == session.project_id);
        let (planning_project_label, planning_project_icon) =
            if project.is_some_and(Project::is_projectless) {
                (tr!("project.chat"), "icons/chat.svg")
            } else {
                (
                    project
                        .map(Project::display_name)
                        .unwrap_or_else(|| tr!("sidebar.unknown_project")),
                    "icons/folder.svg",
                )
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
        let group_name = SharedString::from(format!("boss-planning-row-{id}"));
        let archive_button = session_archivable(session).then(|| {
            let archive_focus = self
                .sidebar_session_archive_focuses
                .borrow_mut()
                .entry(id)
                .or_insert_with(|| cx.focus_handle())
                .clone();
            div()
                .id(SharedString::from(format!("boss-planning-archive-{id}")))
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
                    style.w(px(20.0)).opacity(1.0).bg(theme.focus_highlight())
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
                    this.archive_session(id, window, cx);
                }))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.archive_session(id, window, cx);
                        cx.stop_propagation();
                    }
                }))
        });
        let row = div()
            .id(format!("boss-planning-{id}"))
            .group(group_name.clone())
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
                planning_project_label,
                None,
                Some(planning_project_icon),
                true,
                &theme,
                None,
            ))
            .when_some(status_indicator, |row, indicator| {
                row.child(
                    div()
                        .flex_none()
                        .overflow_hidden()
                        .group_hover(group_name.clone(), |style| style.w_0().opacity(0.0))
                        .child(indicator),
                )
            })
            .when_some(archive_button, |row, button| row.child(button))
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
        let multi_selected = self
            .sidebar_multi_selection
            .contains(&sidebar::SidebarSelection::Deliverable(key, deliverable_id));
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
            // The multi-selection wears an accent wash so it never reads as
            // the armed row's neutral highlight; hover and press deepen it
            // instead of dropping back to the single-selection tint.
            .when(multi_selected, |element| {
                element.bg(theme.accent.opacity(0.14))
            })
            .when(!multi_selected && armed, |element| {
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
            .tooltip(Tooltip::text(
                deliverable
                    .source_path
                    .clone()
                    .unwrap_or_else(|| deliverable.path.clone()),
            ))
            // ⌘-click and ⌘⇧-click select like a task row; a plain click
            // opens the deliverable's task page.
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                let modifiers = event.modifiers();
                if modifiers.secondary() && modifiers.shift {
                    this.extend_sidebar_multi_selection(
                        sidebar::SidebarSelection::Deliverable(key, deliverable_id),
                        cx,
                    );
                } else if modifiers.secondary() {
                    this.toggle_sidebar_multi_selection(
                        sidebar::SidebarSelection::Deliverable(key, deliverable_id),
                        cx,
                    );
                } else {
                    this.open_deliverable_task(key, deliverable_id, cx);
                }
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
            move |cx| {
                let pin_waku = waku.clone();
                let sweep_waku = waku.clone();
                let restore_waku = waku.clone();
                let archive_waku = waku.clone();
                let dismiss_waku = waku.clone();
                // The batch settles at open like a task row's menu: a menu on
                // a member of the multi-selection acts on the whole
                // deliverable batch, while a menu on an outsider drops the
                // set and acts on that row alone. Item callbacks run after
                // the root's plain-click capture has cleared the set, so
                // they close over the resolved list instead of re-reading it.
                let member = sidebar::SidebarSelection::Deliverable(key, deliverable_id);
                let (targets, all_pinned, dormant_targets) = waku
                    .update(cx, |waku, cx| {
                        let targets = if waku.sidebar_multi_selection.contains(&member) {
                            waku.sidebar_deliverable_multi_selection_targets()
                        } else {
                            waku.clear_sidebar_multi_selection(cx);
                            vec![(key, deliverable_id)]
                        };
                        let all_pinned = targets.iter().all(|(key, id)| {
                            waku.boss_deliverable(*key, *id)
                                .is_some_and(|deliverable| deliverable.pinned_at.is_some())
                        });
                        // A pinned record is never dormant — the same flag
                        // the row shows.
                        let dormant_targets = targets
                            .iter()
                            .copied()
                            .filter(|(key, id)| {
                                waku.boss_deliverable(*key, *id).is_some_and(|deliverable| {
                                    deliverable.dormant_at.is_some()
                                        && deliverable.pinned_at.is_none()
                                })
                            })
                            .collect::<Vec<_>>();
                        (targets, all_pinned, dormant_targets)
                    })
                    .unwrap_or((vec![(key, deliverable_id)], pinned, Vec::new()));
                let all_dormant = dormant_targets.len() == targets.len();
                // A batch labels every item with the count it acts on — the
                // multi-selection can hold members the row menu doesn't
                // show (folded rows), so the number is the honest contract.
                let batch = targets.len() > 1;
                let item_label = |single: &'static str, many: &'static str, count: usize| {
                    if batch {
                        tr!(many, count = count)
                    } else {
                        tr!(single).to_string()
                    }
                };
                let sweep_targets = targets
                    .iter()
                    .copied()
                    .filter(|target| !dormant_targets.contains(target))
                    .collect::<Vec<_>>();
                let pin_targets = targets.clone();
                let archive_targets = targets.clone();
                let restore_targets = dormant_targets;
                let dismiss_targets = targets;
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
                        if all_pinned {
                            item_label("session.unpin", "deliverable.unpin_many", pin_targets.len())
                        } else {
                            item_label("session.pin", "deliverable.pin_many", pin_targets.len())
                        },
                        move |_, cx| {
                            let _ = pin_waku.update(cx, |waku, cx| {
                                for (key, id) in pin_targets.iter().copied() {
                                    if waku.boss_deliverable(key, id).is_some_and(|deliverable| {
                                        deliverable.pinned_at.is_some() != !all_pinned
                                    }) {
                                        waku.set_deliverable_pinned(key, id, !all_pinned, cx);
                                    }
                                }
                                // The menu click clears selection in capture phase.
                                // Keep the batch selected so the next menu can undo it.
                                if batch {
                                    waku.sidebar_multi_selection.extend(
                                        pin_targets.iter().copied().map(|(key, id)| {
                                            sidebar::SidebarSelection::Deliverable(key, id)
                                        }),
                                    );
                                    waku.sidebar_multi_selection_anchor = Some(member);
                                    cx.notify();
                                }
                            });
                        },
                    )
                    .shortcut_action(&ToggleSessionPin)
                    .icon(if all_pinned {
                        "icons/pin-off.svg"
                    } else {
                        "icons/pin.svg"
                    }),
                ];
                // Sweep and restore are complementary halves of the same
                // lifecycle, the same split a task row's menu makes: a
                // mixed batch can show both, each acting on the half it
                // applies to.
                if !all_dormant {
                    items.push(
                        MenuItem::new(
                            item_label("session.sweep", "deliverable.sweep_many", sweep_targets.len()),
                            move |_, cx| {
                                let _ = sweep_waku.update(cx, |waku, cx| {
                                    for (key, id) in sweep_targets.iter().copied() {
                                        waku.set_deliverable_dormant(key, id, true, cx);
                                    }
                                });
                            },
                        )
                        .icon("icons/broom.svg"),
                    );
                }
                if !restore_targets.is_empty() {
                    items.push(
                        MenuItem::new(
                            item_label(
                                "session.restore",
                                "deliverable.restore_many",
                                restore_targets.len(),
                            ),
                            move |_, cx| {
                                let _ = restore_waku.update(cx, |waku, cx| {
                                    for (key, id) in restore_targets.iter().copied() {
                                        waku.set_deliverable_dormant(key, id, false, cx);
                                    }
                                });
                            },
                        )
                        .icon("icons/rotate-cw.svg"),
                    );
                }
                items.extend([
                    MenuItem::new(
                        item_label(
                            "session.archive",
                            "deliverable.archive_many",
                            archive_targets.len(),
                        ),
                        move |_, cx| {
                            let _ = archive_waku.update(cx, |waku, cx| {
                                for (key, id) in archive_targets.iter().copied() {
                                    waku.set_deliverable_archived(key, id, true, cx);
                                }
                            });
                        },
                    )
                    .shortcut_action(&ArchiveSession)
                    .icon("icons/archive.svg"),
                    MenuItem::Separator,
                    MenuItem::new(
                        item_label(
                            "deliverable.dismiss",
                            "deliverable.dismiss_many",
                            dismiss_targets.len(),
                        ),
                        move |_, cx| {
                            let _ = dismiss_waku.update(cx, |waku, cx| {
                                for (key, id) in dismiss_targets.iter().copied() {
                                    waku.dismiss_deliverable(key, id, cx);
                                }
                            });
                        },
                    )
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

    /// A previewable local file opens its task page with the file armed as
    /// composer context. Directories and files that cannot be previewed go
    /// to Finder without changing the current app surface.
    pub(super) fn open_deliverable_task(
        &mut self,
        key: DaemonKey,
        deliverable_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        if !self.state.boss_experiment_enabled {
            return;
        }
        let path = self
            .boss_ui
            .states
            .get(&key)
            .and_then(|state| {
                state
                    .deliverables
                    .iter()
                    .find(|deliverable| deliverable.id == deliverable_id)
            })
            .map(|deliverable| (PathBuf::from(&deliverable.path), deliverable.directory));
        if key == DaemonKey::Local
            && let Some((path, recorded_directory)) = path
        {
            let exists = path.try_exists().unwrap_or(false);
            let directory = recorded_directory || path.is_dir();
            let action = deliverable_activation_action(&path, directory, exists && path.is_file());
            if action == DeliverableActivationAction::RevealInFileManager {
                self.mark_deliverable_viewed(key, deliverable_id, cx);
                crate::platform::reveal_in_file_manager(&path, cx);
                if !exists {
                    self.show_toast(tr!(
                        "files.unable_to_edit",
                        error = "the deliverable is unavailable"
                    ));
                    cx.notify();
                }
                return;
            }
        }
        let from = self.navigation_location();
        self.boss_ui
            .forget_unread_deliverable_scroll(key, deliverable_id);
        self.mark_deliverable_viewed(key, deliverable_id, cx);
        // Park before requesting Open: an in-flight Boss request can
        // activate a cached chat synchronously. Keep the outgoing page's
        // arm intact until activation records its history and files its draft.
        self.boss_ui.pending_deliverable = Some((key, deliverable_id, true, from));
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
    pub(super) fn mark_deliverable_viewed(&self, key: DaemonKey, id: Uuid, cx: &mut Context<Self>) {
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

    pub(super) fn mark_employee_viewed_for_session(
        &self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let key = self.boss_ui.states.iter().find_map(|(key, state)| {
            state
                .employees
                .iter()
                .chain(state.retired_employees.iter())
                .find(|employee| {
                    employee.session_id == session_id && employee_attention_is_unread(employee)
                })
                .map(|_| *key)
        });
        let Some(key) = key else {
            return;
        };
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
                        operation: BossOperation::MarkEmployeeViewed { session_id },
                    },
                );
            })
            .detach();
    }

    pub(super) fn toggle_employee_attention_action(
        &mut self,
        _: &ToggleEmployeeAttention,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        // Only active roster entries can be toggled here. Manually archived
        // employees stay archived; the shortcut is for an idle sidebar row.
        let target = self.boss_ui.states.iter().find_map(|(key, state)| {
            state
                .employees
                .iter()
                .find(|employee| employee.session_id == session_id && employee.expired)
                .map(|employee| (*key, !employee.needs_attention))
        });
        let Some((key, needs_attention)) = target else {
            return;
        };
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
                        operation: BossOperation::SetEmployeeAttention {
                            session_id,
                            needs_attention,
                        },
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

    /// The daemon-owned record behind a sidebar deliverable row.
    pub(super) fn boss_deliverable(
        &self,
        key: DaemonKey,
        id: Uuid,
    ) -> Option<&waku_client::boss::BossDeliverable> {
        self.boss_ui
            .states
            .get(&key)
            .and_then(|state| state.deliverables.iter().find(|d| d.id == id))
    }

    /// A deliverable whose row left the sidebar — archived or dismissed —
    /// drops out of the multi-selection the way a departed task does.
    fn prune_deliverable_multi_selection(&mut self, key: DaemonKey, id: Uuid) {
        let member = sidebar::SidebarSelection::Deliverable(key, id);
        self.sidebar_multi_selection.remove(&member);
        if self.sidebar_multi_selection_anchor == Some(member) {
            self.sidebar_multi_selection_anchor = None;
        }
    }

    /// Task affordances on a deliverable row are daemon mutations — the deliverable
    /// record is boss state, so the pin, sweep, and archive the row shows
    /// round-trip through the owning daemon like a publish or dismiss.
    pub(super) fn set_deliverable_pinned(
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

    pub(super) fn set_deliverable_archived(
        &mut self,
        key: DaemonKey,
        id: Uuid,
        archived: bool,
        cx: &mut Context<Self>,
    ) {
        if archived {
            self.prune_deliverable_multi_selection(key, id);
        }
        self.boss_request(
            key,
            BossOperation::ArchiveDeliverable { id, archived },
            BossReply::List,
            cx,
        );
    }

    /// Dismiss removes the record outright — the same prune an archive
    /// applies, then the daemon call that deletes it.
    fn dismiss_deliverable(&mut self, key: DaemonKey, id: Uuid, cx: &mut Context<Self>) {
        self.prune_deliverable_multi_selection(key, id);
        self.boss_request(
            key,
            BossOperation::DismissDeliverable { id },
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
    /// for whichever column width the caller wraps it in. On the Memory tab
    /// the rows' painted text also registers for selection and note
    /// pinning: a zero-size reset clears the frame's registry ahead of the
    /// rows and a hit-region canvas installs the listeners.
    fn boss_item_list(&self, cx: &mut Context<Self>) -> Div {
        let visible_rows = self.boss_ui.rows.clone();
        let weak = cx.entity().downgrade();
        let on_memory = self.boss_ui.page.map(|(_, tab)| tab) == Some(BossTab::Memory);
        let frame_reset = on_memory.then(|| {
            md::render::frame_reset(self.memory_feedback_selection()).into_any_element()
        });
        let listeners = on_memory.then(|| {
            let selection = self.memory_feedback_selection();
            let weak = cx.entity().downgrade();
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal).id,
                move |_, region, window, cx| {
                    md::render::install_selection_input(region, window, &selection, None);
                    Waku::install_memory_annotation_input(region, window, cx, &selection, &weak);
                },
            )
            .absolute()
            .inset_0()
            .size_full()
            .into_any_element()
        });
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .when_some(frame_reset, |element, reset| element.child(reset))
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
            .when_some(listeners, |element, listeners| {
                element.child(listeners)
            })
    }

    // ── Memory ───────────────────────────────────────────────────────────

    /// The Memory section: one unified feed of original notes across every
    /// accessible bucket — archive-style search and project filter on top,
    /// newest-first rows beneath. Buckets survive as row labels only.
    fn render_boss_memory_section(
        &mut self,
        key: DaemonKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        // A Review "Show memory" asks the feed to reveal its record: clear
        // whatever browse state hides it, reveal the row, scroll it into
        // view.
        if let Some(locate) = self.consume_memory_feedback_locate(key) {
            self.boss_memory_search
                .update(cx, |input, cx| input.set_content(String::new(), cx));
            self.boss_ui.memory_query.clear();
            self.boss_ui.memory_project.remove(&key);
            if let Some(feed) = self.boss_ui.memory_feed.get_mut(&key)
                && let Some(index) = feed.records.iter().position(|record| {
                    record.bucket_id == locate.bucket && record.id == locate.reference
                })
            {
                feed.visible = feed.visible.max(index + 1);
                feed.filter_notice = None;
                self.sync_boss_page_rows();
                if let Some(row_index) = self.boss_ui.rows.iter().position(|item| {
                    matches!(item, BossItem::MemoryRecord(i) if *i == index)
                }) {
                    self.boss_ui.list.scroll_to(gpui::ListOffset {
                        item_ix: row_index,
                        offset_in_item: px(0.0),
                    });
                }
            }
        }
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
        // The records the list actually renders right now — the pending
        // count's filter warning is measured against them.
        let visible_keys: HashSet<memory_annotations::MemoryAnnotationKey> = self
            .boss_ui
            .rows
            .iter()
            .filter_map(|item| match item {
                BossItem::MemoryRecord(index) => self
                    .boss_ui
                    .memory_feed
                    .get(&key)
                    .and_then(|feed| feed.records.get(*index)),
                _ => None,
            })
            .map(|record| memory_annotations::MemoryAnnotationKey {
                daemon: key,
                bucket: record.bucket_id.clone(),
                reference: record.id.clone(),
            })
            .collect();
        let supplement = self.render_memory_feedback_supplement(
            window,
            move |key| visible_keys.contains(key),
            cx,
        );
        let offer = self.render_memory_annotation_offer(cx);
        let editor = self.render_memory_annotation_editor(cx);
        let tooltip = self.render_memory_annotation_tooltip(cx);
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
            .when_some(supplement, |element, supplement| element.child(supplement))
            .children([offer, editor, tooltip].into_iter().flatten())
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

    /// A full-width Employees row — the shared employee card painted from
    /// `boss_employee_row_data`; avatar and name over the employee detail, with
    /// a readable status and the summon or finish age on
    /// the right. Blocked and queued reasons get their own line.
    fn render_boss_employee_page_row(
        &self,
        id: Uuid,
        key: DaemonKey,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.boss_employee_row_data(id, key, cx)
            .map(|row| boss_employee_row_element(&row, &cx.entity().downgrade(), cx))
            .unwrap_or_else(|| div().into_any_element())
    }

    /// The Employees-tab row's prepared paint data — roster record joined
    /// with the cached session, identity, job, and queue lookups, plus
    /// the plan chip's resolved state.
    fn boss_employee_row_data(
        &self,
        id: Uuid,
        key: DaemonKey,
        cx: &App,
    ) -> Option<BossEmployeeRow> {
        let theme = Theme::current(cx);
        let identity = self.boss_ui.identities.get(&id)?.clone();
        let state = self.boss_ui.states.get(&key);
        let employee = state.and_then(|state| {
            state
                .employees
                .iter()
                .chain(state.retired_employees.iter())
                .find(|employee| employee.session_id == id)
        });
        let session = self.state.sessions.iter().find(|session| session.id == id);
        let detail = self
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
        let project = project.filter(|project| !project.is_empty());
        let mut reasons = Vec::new();
        if let Some(employee) = employee {
            if let Some(blocker) = employee
                .blocker
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                reasons.push(BossEmployeeReason::Blocked(blocker.to_owned()));
            }
            if let Some(wait) = self.boss_ui.queued.get(&id) {
                reasons.push(BossEmployeeReason::Queued(wait.clone()));
            }
        }
        let readiness = employee.and_then(boss_plan_readiness_label);
        let status =
            employee.map(|employee| self.boss_employee_status_label(employee, session, &theme));
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
            .map(crate::custom_commands::icon_path)
            .unwrap_or_else(|| job_title_icon(&detail));
        Some(BossEmployeeRow {
            slot: "page".to_owned(),
            session_id: id,
            key,
            avatar: self.prepare_boss_employee_avatar(&identity, BOSS_EMPLOYEE_AVATAR, cx),
            name: identity.name.clone(),
            detail,
            job_icon,
            project,
            stamp,
            now: unix_time(),
            reasons,
            readiness,
            status,
            plan_recovery: employee.and_then(|employee| self.employee_plan_recovery(key, employee)),
            focus: None,
            navigable: true,
            nested: false,
            aria: None,
        })
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
                // The admission wait itself is the row's own-line reason —
                // the status word stays bare so it does not duplicate it.
                (tr!("boss.goals_status_queued"), theme.text_secondary)
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

    /// The roster record used by the employee header's plan controls —
    /// live-roster employees and retired entries their transcripts still
    /// resolve to, paired with the daemon that owns the record.
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

    /// The required plan's chip state for an employee — `None` when the
    /// record binds no plan; the Open target resolves only while the
    /// plan's own session still exists.
    pub(super) fn employee_plan_recovery(
        &self,
        key: DaemonKey,
        employee: &waku_protocol::boss::BossEmployee,
    ) -> Option<BossEmployeePlanRecovery> {
        let binding = employee.plan_binding.as_ref()?;
        let retry = employee
            .plan_readiness
            .as_ref()
            .is_some_and(|r| r.phase == waku_protocol::boss::PlanReadinessPhase::Blocked);
        let open = self.boss_ui.states.get(&key).and_then(|state| {
            state
                .planning
                .iter()
                .find(|plan| plan.id == binding.plan_id)
                .map(|plan| plan.session_id)
        });
        Some(BossEmployeePlanRecovery { open, retry })
    }

    #[track_caller]
    fn employee_plan_recovery_controls(
        &self,
        key: DaemonKey,
        employee: &waku_protocol::boss::BossEmployee,
        slot: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.employee_plan_recovery(key, employee).map(|recovery| {
            boss_employee_plan_chips(
                &cx.entity().downgrade(),
                key,
                employee.session_id,
                &recovery,
                slot,
                theme,
            )
        })
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
        let rows = Rc::new(self.employee_assignment_rows(state, employee));
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
    /// label/value pairs for the brief, persona, access, and resource
    /// state the summon recorded.
    fn employee_assignment_rows(
        &self,
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
        if employee.boss_authority {
            access.push(tr!("boss.authority"));
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
        let mut rows: Vec<(String, String)> = [
            brief.map(|brief| (tr!("boss.assignment_brief"), brief)),
            persona_name.map(|name| (tr!("boss.assignment_persona"), name)),
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
        .collect();
        if let Some(label) = boss_plan_readiness_label(employee) {
            rows.push((tr!("boss.plan_readiness"), label));
        }
        if let Some(binding) = &employee.plan_binding {
            rows.push((
                tr!("boss.plan_binding"),
                format!(
                    "{} / {} / {}",
                    binding.owner_id,
                    binding.plan_id,
                    binding.content_digest.as_deref().unwrap_or("—")
                ),
            ));
        }
        if let Some(readiness) = &employee.plan_readiness {
            rows.push((
                tr!("boss.plan_delivery"),
                tr!(
                    "boss.plan_delivery_detail",
                    delivered = readiness.delivered
                ),
            ));
            rows.push((
                tr!("boss.plan_attempt"),
                format!("{} / {}", readiness.attempt, readiness.runtime_generation),
            ));
            if let Some(error) = &readiness.failure {
                rows.push((tr!("boss.plan_recovery"), error.detail.clone()));
            }
            if let Some(notice) = employee
                .plan_blockers
                .iter()
                .rev()
                .find(|n| n.attempt == readiness.attempt)
            {
                rows.push((
                    tr!("boss.plan_blocker_receipt"),
                    if notice.supervisor_notified_at.is_some() {
                        tr!("boss.plan_blocker_notified")
                    } else {
                        tr!("boss.plan_blocker_recorded")
                    },
                ));
            }
        }
        if let Some(admission) = boss_queue_detail(employee, None) {
            rows.push((tr!("boss.plan_admission"), admission));
        }
        rows
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
        let mut rows: Vec<(String, AnyElement)> = vec![
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
        if persona.id != state.persona_id {
            rows.push((
                tr!("boss.persona_default_purpose_label"),
                boss_plain_value(
                    &theme,
                    match persona.default_purpose {
                        Some(waku_protocol::boss::EmployeePurpose::Building) => {
                            tr!("boss.purpose_building")
                        }
                        Some(waku_protocol::boss::EmployeePurpose::Helping) => {
                            tr!("boss.purpose_helping")
                        }
                        None => tr!("boss.detail_none"),
                    },
                ),
            ));
        }
        let count = rows.len();
        let mut info = div().mt(px(14.0)).flex().flex_col();
        for (index, (label, value)) in rows.into_iter().enumerate() {
            info = info.child(boss_info_row(&theme, label, value, index + 1 == count));
        }
        let palette = MarkdownPalette::from_theme(&theme);
        let layers = state.persona_instruction_layers(persona.id);
        let document: Option<AnyElement> = (!layers.is_empty()).then(|| {
            let mut cache = self.boss_ui.persona_markdown.borrow_mut();
            if !matches!(cache.as_ref(), Some((cached, _)) if *cached == persona.id) {
                *cache = Some((persona.id, MarkdownView::document()));
            }
            let (_, view) = cache.as_mut().expect("entry ensured above");
            view.set_text(&persona_layers_document(&layers), false);
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
                            .when(default_role.is_none() && !boss_role, |header| {
                                let persona_id = persona.id;
                                let confirming = self.boss_ui.persona_delete_confirmation
                                    == Some((key, persona_id));
                                let label = if confirming {
                                    tr!("boss.persona_delete_confirm")
                                } else {
                                    tr!("boss.persona_delete")
                                };
                                header.child(
                                    boss_button("boss-delete-persona", label.clone(), &theme)
                                        .child(icon(
                                            "icons/trash.svg",
                                            13.0,
                                            theme.text_secondary,
                                        ))
                                        .child(label)
                                        .on_activation(cx, move |this, _, cx| {
                                            if confirming {
                                                this.boss_ui.persona_delete_confirmation = None;
                                                this.boss_request(
                                                    key,
                                                    BossOperation::DeletePersona { persona_id },
                                                    BossReply::Saved,
                                                    cx,
                                                );
                                            } else {
                                                this.boss_ui.persona_delete_confirmation =
                                                    Some((key, persona_id));
                                                cx.notify();
                                            }
                                        }),
                                ).when(confirming, |header| {
                                    header.child(
                                        boss_button("boss-delete-persona-cancel", tr!("common.cancel"), &theme)
                                            .child(tr!("common.cancel"))
                                            .on_activation(cx, |this, _, cx| {
                                                this.boss_ui.persona_delete_confirmation = None;
                                                cx.notify();
                                            }),
                                    )
                                })
                            })
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
                    .children(document),
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
        let mut header = div()
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
        if plan.finalized_at.is_some() && plan.approved_revisions.is_empty() {
            header = header.child(
                boss_button("boss-plan-reconfirm", tr!("boss.plan_reconfirm"), &theme)
                    .child(tr!("boss.plan_reconfirm"))
                    .on_activation(cx, move |this, _, cx| {
                        this.request_plan_reconfirmation(key, session_id, cx);
                    }),
            );
        }
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
                            waku.dismiss_deliverable(key, deliverable_id, cx);
                        });
                    })
                    .icon("icons/trash.svg"),
                );
                items
            },
        )
        .into_any_element()
    }

    fn render_boss_item(&mut self, item: BossItem, cx: &mut Context<Self>) -> AnyElement {
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
    /// text beneath as selectable markdown, the "Add note" affordance at
    /// the row's right edge, and any pending notes pinned on the record
    /// beneath that. Read-only; feedback gathers for Boss chat rather than
    /// editing.
    fn render_boss_memory_record(
        &mut self,
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
        let record_ref = memory_annotations::MemoryRecordRef {
            key: memory_annotations::MemoryAnnotationKey {
                daemon: key,
                bucket: record.bucket_id.clone(),
                reference: record.id.clone(),
            },
            sequence: record.sequence,
            bucket: record.bucket.clone(),
            project: record.project.clone(),
            created_at: record.created_at,
            text: Rc::from(record.text.as_str()),
        };
        let annotate_name = format!(
            "{} — {}",
            tr!("memory.add_note_to_memory"),
            record.text.chars().take(80).collect::<String>()
        );
        // Painted text registers under the record's stable key so a settled
        // selection or a pinned highlight resolves to the record, never the
        // row's position.
        let selection = self.memory_feedback_selection();
        let palette = MarkdownPalette::from_theme(&theme);
        let ctx = MarkdownCtx::new(
            memory_annotations::memory_row_key(&record_ref.key),
            &palette,
            MarkdownMetrics::document(self.state.ui_font_size, self.state.code_font_size),
            selection,
        )
        .with_families(crate::fonts::current(cx));
        let document = {
            let view = self
                .memory_feedback
                .record_views
                .entry(record_ref.key.clone())
                .or_insert_with(MarkdownView::document);
            view.set_text(&record.text, false);
            md::render::markdown(view, &ctx)
        };
        let annotate = self.render_memory_record_annotate_button(
            &record_ref,
            &group,
            annotate_name,
            &theme,
            cx,
        );
        let notes = self.render_memory_record_notes(&record_ref, cx);
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
                    .child(annotate),
            )
            .child(
                div()
                    .pt(px(4.0))
                    .text_size(sp(13.0))
                    .line_height(sp(19.0))
                    .when(!expanded, |element| {
                        element
                            .max_h(px(MEMORY_EXPAND_LINES as f32 * 19.0))
                            .overflow_hidden()
                    })
                    .children(document),
            )
            .children(notes);
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
        let canonical = self
            .boss_ui
            .states
            .get(&editor.key)
            .and_then(|state| state.persona_default_role(editor.persona))
            .is_some();
        let mut form = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(tr!("boss.name_path"))
            .child(boss_input(editor.name.clone(), &theme))
            .child(if canonical {
                tr!("boss.markdown_customization")
            } else {
                tr!("boss.markdown")
            })
            .when(canonical, |form| {
                form.child(
                    div()
                        .text_size(sp(12.0))
                        .text_color(theme.text_tertiary)
                        .child(tr!("boss.markdown_customization_hint")),
                )
            })
            .child(boss_input(editor.content.clone(), &theme))
            .child(tr!("boss.persona_buckets"))
            .child(boss_input(editor.buckets.clone(), &theme))
            .child(tr!("boss.pinned_files"))
            .child(boss_input(editor.pinned.clone(), &theme))
            .when(
                self.boss_ui
                    .states
                    .get(&editor.key)
                    .is_some_and(|state| state.persona_id != editor.persona),
                |form| {
                    form.child(tr!("boss.persona_default_purpose")).child(
                        div().flex().gap(px(4.0)).children(
                            [
                                (None, tr!("boss.detail_none")),
                                (
                                    Some(waku_protocol::boss::EmployeePurpose::Building),
                                    tr!("boss.purpose_building"),
                                ),
                                (
                                    Some(waku_protocol::boss::EmployeePurpose::Helping),
                                    tr!("boss.purpose_helping"),
                                ),
                            ]
                            .into_iter()
                            .map(|(choice, label)| {
                                boss_button(
                                    format!("persona-purpose-{label}"),
                                    label.clone(),
                                    &theme,
                                )
                                .child(if editor.default_purpose == choice {
                                    format!("✓ {label}")
                                } else {
                                    label
                                })
                                .on_activation(
                                    cx,
                                    move |this, _, cx| {
                                        if let Some(editor) = &mut this.boss_ui.editor {
                                            editor.default_purpose = choice;
                                        }
                                        cx.notify();
                                    },
                                )
                            }),
                        ),
                    )
                },
            )
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeliverableActivationAction {
    Preview,
    RevealInFileManager,
}

pub(super) fn deliverable_activation_action(
    path: &Path,
    directory: bool,
    is_file: bool,
) -> DeliverableActivationAction {
    if directory || !is_file || !right_panel::transfer_file_is_previewable(path) {
        DeliverableActivationAction::RevealInFileManager
    } else {
        DeliverableActivationAction::Preview
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

/// An `Open` reply lands on whatever surface the user had when the request
/// went out. If they have since navigated — a project-switcher commit, a
/// sidebar click — the open's intent is stale and the reply must not
/// activate the chat over the surface they chose.
fn boss_open_reply_navigates(open_from: Option<Uuid>, selected: Option<Uuid>) -> bool {
    open_from == selected
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

/// The queue group a ticket's position counts in — the daemon's
/// `queue_root`: walk `supervisor_id` up the employee chain until the
/// parent is not an employee. Subordinates queue under the same root as
/// their supervisor's summon.
fn boss_queue_root(state: &BossState, employee: &BossEmployee) -> Uuid {
    let mut current = employee.supervisor_id;
    for _ in 0..state.employees.len() {
        let Some(parent) = state
            .employees
            .iter()
            .find(|entry| entry.session_id == current)
        else {
            return current;
        };
        current = parent.supervisor_id;
    }
    current
}

/// The employee's 1-based position inside its own queue root and the root's
/// queued count — the card's "Position N of M". Mirrors the daemon's
/// `queue_position`: sequence order within the group, `None` when the
/// employee is not waiting.
fn boss_queue_position(state: &BossState, session_id: Uuid) -> Option<(usize, usize)> {
    let employee = state
        .employees
        .iter()
        .find(|entry| entry.session_id == session_id)?;
    if employee.lifecycle() != waku_protocol::boss::EmployeeLifecycle::Queued {
        return None;
    }
    let root = boss_queue_root(state, employee);
    let sequence = employee.ticket.as_ref()?.sequence;
    let mut position = 0;
    let mut total = 0;
    for entry in &state.employees {
        if entry.lifecycle() == waku_protocol::boss::EmployeeLifecycle::Queued
            && boss_queue_root(state, entry) == root
        {
            total += 1;
            if entry
                .ticket
                .as_ref()
                .is_some_and(|ticket| ticket.sequence <= sequence)
            {
                position += 1;
            }
        }
    }
    (position > 0).then_some((position, total))
}

/// The truthful state the queued card renders — the plan's
/// `Queued → Starting → gone, or back to Queued` cycle. `Waiting` waits on
/// admission; `Starting` carries a committed `startNow` that dispatch has
/// not settled; `Declined` is a committed start the broker still cannot
/// grant — the card re-arms the action rather than pretending success.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QueuedCardPhase {
    Waiting,
    Starting,
    Declined,
}

/// Everything the tail row paints, derived in one place so the renderer
/// stays a stencil. `key` routes the control op to the owning daemon.
pub(super) struct QueuedEmployeeCard {
    pub key: DaemonKey,
    pub session_id: Uuid,
    pub phase: QueuedCardPhase,
    pub headline: String,
    pub secondary: Option<String>,
}

/// The card's phase and copy for a queued ticket — blocker kind chooses
/// the headline; position and slot counts land on the secondary line only
/// when they help the reader act.
fn queued_card_copy(
    employee: &BossEmployee,
    position: Option<(usize, usize)>,
    start_pending: bool,
) -> (QueuedCardPhase, String, Option<String>) {
    use waku_protocol::boss::AdmissionBlocker;
    let ticket = employee.ticket.as_ref();
    let blocker = ticket.and_then(|ticket| {
        ticket
            .blocked_by
            .iter()
            .find(|blocker| !matches!(blocker, AdmissionBlocker::Plan { .. }))
            .or_else(|| ticket.blocked_by.first())
    });
    let reason = || match blocker {
        Some(AdmissionBlocker::ModelLimit { used, limit }) => {
            tr!("boss.queue_card_slots_in_use", used = used, limit = limit)
        }
        Some(
            AdmissionBlocker::HostResources { detail }
            | AdmissionBlocker::OutcomeWait { detail }
            | AdmissionBlocker::EmployeeBase { detail },
        ) => detail.clone(),
        Some(AdmissionBlocker::Plan { error }) => error.detail.clone(),
        None => tr!("boss.goals_queue_admission"),
    };
    let forced = ticket.is_some_and(|ticket| ticket.force_start) || start_pending;
    if forced && blocker.is_some() {
        return (
            QueuedCardPhase::Declined,
            tr!("boss.queue_card_still_waiting", reason = reason()),
            None,
        );
    }
    if forced {
        return (
            QueuedCardPhase::Starting,
            tr!("boss.goals_status_starting"),
            None,
        );
    }
    let position = position
        .filter(|(rank, _)| *rank > 1)
        .map(|(rank, total)| tr!("boss.queue_card_position", position = rank, total = total));
    let (headline, secondary) = match blocker {
        Some(AdmissionBlocker::ModelLimit { used, limit }) => (
            tr!("boss.queue_card_waiting_slot"),
            Some(position.unwrap_or_else(|| {
                tr!("boss.queue_card_slots_in_use", used = used, limit = limit)
            })),
        ),
        Some(AdmissionBlocker::HostResources { .. }) => {
            (tr!("boss.queue_card_waiting_resources"), Some(reason()))
        }
        Some(_) => (tr!("boss.goals_status_queued"), Some(reason())),
        None => (tr!("boss.goals_status_queued"), position),
    };
    (QueuedCardPhase::Waiting, headline, secondary)
}

/// The shared employee card's geometry — the Employees tab's 42px row,
/// plus a line per own-line reason and its 24px face.
const BOSS_EMPLOYEE_ROW_HEIGHT: f32 = 42.0;
const BOSS_EMPLOYEE_ROW_REASON_HEIGHT: f32 = 18.0;
pub(super) const BOSS_EMPLOYEE_AVATAR: f32 = 24.0;

/// The card's face — the same Gaze/still/placeholder `boss_avatar` and
/// `boss_avatar_animated` produce, painted from prepared rasters.
fn boss_employee_avatar_element(avatar: &BossEmployeeAvatar, theme: &Theme) -> AnyElement {
    match avatar {
        BossEmployeeAvatar::Gaze { faces, shift } => {
            let faces = faces.clone();
            let shift = *shift;
            motion::pulse_elapsed(move |elapsed| {
                gaze_eye_frame(&faces, BOSS_EMPLOYEE_AVATAR, elapsed, shift)
            })
            .every(2)
            .into_any_element()
        }
        BossEmployeeAvatar::Still {
            image,
            circular,
            initial,
        } => {
            if let Some(image) = image {
                let image = gpui::img(image.clone()).size(px(BOSS_EMPLOYEE_AVATAR));
                return if *circular {
                    image.rounded_full().into_any_element()
                } else {
                    image.rounded(px(6.0)).into_any_element()
                };
            }
            let placeholder = div().size(px(BOSS_EMPLOYEE_AVATAR));
            let placeholder = if *circular {
                placeholder.rounded_full()
            } else {
                placeholder.rounded(px(6.0))
            };
            placeholder
                .bg(theme.overlay)
                .flex()
                .items_center()
                .justify_center()
                .child(initial.clone())
                .into_any_element()
        }
    }
}

/// The bound plan's chips — Open the plan's session while it resolves,
/// and Retry while its review is blocked.
#[track_caller]
fn boss_employee_plan_chips(
    waku: &WeakEntity<Waku>,
    key: DaemonKey,
    session_id: Uuid,
    recovery: &BossEmployeePlanRecovery,
    slot: &str,
    theme: &Theme,
) -> AnyElement {
    let mut controls = div().flex().gap(px(4.0));
    if let Some(plan_session) = recovery.open {
        let open_waku = waku.clone();
        controls = controls.child(
            div()
                .id(SharedString::from(format!(
                    "required-plan-open-{slot}-{session_id}"
                )))
                .tab_index(0)
                .px(px(7.0))
                .h(px(22.0))
                .rounded(px(8.0))
                .bg(theme.overlay)
                .text_size(sp(12.0))
                .text_color(theme.text_secondary)
                .cursor_pointer()
                .focus_visible(|s| s.bg(theme.focus_highlight()))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_activation_app(move |_, cx| {
                    let _ = open_waku.update(cx, |this, cx| {
                        this.request_session_activation(
                            plan_session,
                            SessionActivationTransition::Visit,
                            cx,
                        )
                    });
                })
                .child(tr!("boss.plan_open_bound")),
        );
    }
    if recovery.retry {
        let retry_waku = waku.clone();
        controls = controls.child(
            div()
                .id(SharedString::from(format!(
                    "required-plan-retry-{slot}-{session_id}"
                )))
                .tab_index(0)
                .px(px(7.0))
                .h(px(22.0))
                .rounded(px(8.0))
                .bg(theme.overlay)
                .text_size(sp(12.0))
                .text_color(theme.text_secondary)
                .cursor_pointer()
                .focus_visible(|s| s.bg(theme.focus_highlight()))
                .tooltip(Tooltip::text(tr!("boss.plan_retry_hint")))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_activation_app(move |_, cx| {
                    let _ = retry_waku.update(cx, |this, cx| {
                        if this.boss_ui.pending {
                            this.show_toast(tr!("boss.loading"));
                            cx.notify();
                            return;
                        }
                        this.boss_request(
                            key,
                            BossOperation::RetryPlanReview { session_id },
                            BossReply::Resume,
                            cx,
                        )
                    });
                })
                .child(tr!("boss.plan_retry")),
        );
    }
    controls.into_any_element()
}

/// The employee card the Employees tab and an expanded outcome's member
/// list share — avatar, name over the row detail with its project and
/// stamp, each blocked/queued reason on its own line, the status, and the
/// action chips the caller resolved. `waku` backs activation
/// and chip dispatch; callers holding `Context<Waku>` hand over
/// `&cx.entity().downgrade()`.
#[track_caller]
pub(super) fn boss_employee_row_element(
    row: &BossEmployeeRow,
    waku: &WeakEntity<Waku>,
    cx: &App,
) -> AnyElement {
    let theme = Theme::current(cx);
    let session_id = row.session_id;
    let key = row.key;
    let height =
        BOSS_EMPLOYEE_ROW_HEIGHT + row.reasons.len() as f32 * BOSS_EMPLOYEE_ROW_REASON_HEIGHT;
    div()
        .id(SharedString::from(format!(
            "boss-employee-row-{}-{session_id}",
            row.slot
        )))
        .when_some(row.focus.clone(), |element, focus| {
            element.track_focus(&focus)
        })
        .when(row.focus.is_none() && row.navigable, |element| {
            element.tab_index(0)
        })
        .h(px(height))
        .w_full()
        .pl(px(if row.nested { 26.0 } else { 8.0 }))
        .pr(px(8.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded(px(6.0))
        .when_some(row.aria.clone(), |element, aria| element.aria_label(aria))
        .when(row.navigable, |element| {
            let activate = waku.clone();
            element
                .cursor_pointer()
                .hover(|style| style.bg(theme.overlay))
                .focus_visible(|style| style.bg(theme.focus_highlight()))
                .on_activation_app(move |_, cx| {
                    let _ = activate.update(cx, |this, cx| {
                        this.request_session_activation(
                            session_id,
                            SessionActivationTransition::Visit,
                            cx,
                        );
                    });
                })
        })
        .child(boss_employee_avatar_element(&row.avatar, &theme))
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
                        .min_w_0()
                        .gap(px(6.0))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_size(sp(14.0))
                                .line_height(sp(17.0))
                                .text_color(theme.text)
                                .truncate()
                                .child(row.name.clone()),
                        )
                        .when_some(row.project.clone(), |line, project| {
                            line.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .min_w_0()
                                    .gap(px(4.0))
                                    .text_size(sp(12.0))
                                    .line_height(sp(15.0))
                                    .text_color(theme.text_tertiary)
                                    .child(icon("icons/folder.svg", 12.0, theme.text_tertiary))
                                    .child(div().min_w_0().truncate().child(project)),
                            )
                        })
                        .when_some(row.stamp, |line, stamp| {
                            line.child(
                                div()
                                    .flex_none()
                                    .ml_auto()
                                    .w(px(72.0))
                                    .text_right()
                                    .text_size(sp(12.0))
                                    .text_color(theme.text_ghost)
                                    .child(sidebar::format_time_ago(row.now.saturating_sub(stamp))),
                            )
                        }),
                )
                .when(!row.detail.is_empty(), |text| {
                    text.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .min_w_0()
                            .text_size(sp(13.0))
                            .line_height(sp(15.0))
                            .text_color(theme.text_tertiary)
                            .child(icon(row.job_icon, 12.0, theme.text_tertiary))
                            .child(div().min_w_0().truncate().child(row.detail.clone())),
                    )
                })
                .children(row.reasons.iter().map(|reason| {
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(sp(12.0))
                        .line_height(sp(15.0))
                        .text_color(reason.color(&theme))
                        .child(reason.text().to_owned())
                })),
        )
        .when_some(row.readiness.clone(), |element, readiness| {
            element.child(
                div()
                    .max_w(px(280.0))
                    .truncate()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(readiness),
            )
        })
        .when_some(
            row.plan_recovery.as_ref().map(|recovery| {
                boss_employee_plan_chips(waku, key, session_id, recovery, &row.slot, &theme)
            }),
            |element, chips| element.child(chips),
        )
        .when_some(row.status.clone(), |element, (label, color)| {
            element.child(
                div()
                    .flex_none()
                    .max_w(px(280.0))
                    .truncate()
                    .text_size(sp(12.0))
                    .text_color(color)
                    .child(label),
            )
        })
        .into_any_element()
}

/// Keep required-plan progression separate from admission and never reuse a
/// previous attempt's delivery evidence while a fresh ticket waits.
pub(super) fn boss_plan_readiness_label(
    employee: &waku_protocol::boss::BossEmployee,
) -> Option<String> {
    use waku_protocol::boss::{PlanAssignmentKind, PlanReadinessPhase};
    if employee.assignment_kind == PlanAssignmentKind::Research && employee.plan_id.is_some() {
        return Some(tr!("boss.plan_research_only"));
    }
    if employee.plan_binding.is_none() && employee.plan_id.is_none() {
        return None;
    }
    let phase = if employee.ticket.as_ref().is_some_and(|ticket| {
        ticket
            .blocked_by
            .iter()
            .any(|b| matches!(b, waku_protocol::boss::AdmissionBlocker::Plan { .. }))
    }) {
        tr!("boss.plan_blocked")
    } else {
        match employee.plan_readiness.as_ref().map(|r| r.phase) {
            Some(PlanReadinessPhase::Blocked) => tr!("boss.plan_blocked"),
            Some(PlanReadinessPhase::Delivered) | Some(PlanReadinessPhase::Ready) => {
                tr!("boss.plan_delivered")
            }
            _ => tr!("boss.plan_preparing"),
        }
    };
    Some(tr!(
        "boss.plan_readiness_summary",
        phase = phase,
        revision = employee
            .plan_binding
            .as_ref()
            .and_then(|binding| binding.revision_id)
            .map(|id| id.to_string())
            .unwrap_or_else(|| tr!("boss.plan_revision_unverified"))
    ))
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
    let detail = match employee.ticket.as_ref().and_then(|ticket| {
        ticket
            .blocked_by
            .iter()
            .find(|blocker| !matches!(blocker, waku_protocol::boss::AdmissionBlocker::Plan { .. }))
    }) {
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
        Some(waku_protocol::boss::AdmissionBlocker::Plan { error }) => error.detail.clone(),
        Some(waku_protocol::boss::AdmissionBlocker::HostResources { detail })
        | Some(waku_protocol::boss::AdmissionBlocker::OutcomeWait { detail })
        | Some(waku_protocol::boss::AdmissionBlocker::EmployeeBase { detail }) => detail.clone(),
        None => {
            if employee.ticket.as_ref().is_some_and(|ticket| {
                ticket
                    .blocked_by
                    .iter()
                    .any(|b| matches!(b, waku_protocol::boss::AdmissionBlocker::Plan { .. }))
            }) {
                tr!("boss.plan_admission_not_evaluated")
            } else if rank == Some(1) {
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
    revealed_project: Option<String>,
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
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(sp(14.0))
                .line_height(sp(17.0))
                .child(
                    div()
                        .when(revealed_project.is_some(), |name| name.flex_none())
                        .when(revealed_project.is_none(), |name| {
                            name.min_w_0().flex_1().truncate()
                        })
                        .text_color(theme.text)
                        .child(name),
                )
                .when_some(revealed_project, |line, project| {
                    line.child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(12.0))
                            .line_height(sp(15.0))
                            .text_color(theme.text_tertiary)
                            .child(project),
                    )
                }),
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

/// One row of the composer's target menu: the cached face or lettered
/// placeholder, the recipient's name over an optional detail line (job
/// title, "Finished"), and a trailing check on the live pick. Drawn from
/// captured state — menu rows render inside the card's own paint, where
/// the entity is already borrowed.
fn target_row(
    face: Option<Arc<gpui::RenderImage>>,
    identity: &BossIdentity,
    name: &str,
    detail: Option<String>,
    selected: bool,
    theme: &Theme,
) -> AnyElement {
    let circular = boss_moods::circular_frame(identity.avatar_style);
    let avatar: AnyElement = if let Some(image) = face {
        let image = gpui::img(image).size(px(18.0));
        if circular {
            image.rounded_full().into_any_element()
        } else {
            image.rounded(px(6.0)).into_any_element()
        }
    } else {
        let placeholder = div().size(px(18.0));
        let placeholder = if circular {
            placeholder.rounded_full()
        } else {
            placeholder.rounded(px(6.0))
        };
        placeholder
            .bg(theme.overlay)
            .flex()
            .items_center()
            .justify_center()
            .text_size(sp(10.0))
            .text_color(theme.text_tertiary)
            .child(name.chars().next().unwrap_or('B').to_string())
            .into_any_element()
    };
    div()
        .w(px(260.0))
        .py(px(4.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .child(avatar)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(sp(12.5))
                        .font_weight(if selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(theme.text)
                        .child(name.to_owned()),
                )
                .when_some(detail, |element, detail| {
                    element.child(
                        div()
                            .w_full()
                            .mt(px(2.0))
                            .truncate()
                            .text_size(sp(11.5))
                            .line_height(sp(13.0))
                            .text_color(theme.text_tertiary)
                            .child(detail),
                    )
                }),
        )
        .when(selected, |element| {
            element.child(icon("icons/check.svg", 11.0, theme.text_tertiary))
        })
        .into_any_element()
}

#[track_caller]
pub(super) fn boss_button(
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

    #[test]
    fn deliverable_activation_only_navigates_for_previewable_files() {
        assert_eq!(
            deliverable_activation_action(Path::new("report.md"), false, true),
            DeliverableActivationAction::Preview
        );
        assert_eq!(
            deliverable_activation_action(Path::new("archive.zip"), false, true),
            DeliverableActivationAction::RevealInFileManager
        );
        assert_eq!(
            deliverable_activation_action(Path::new("folder.md"), true, false),
            DeliverableActivationAction::RevealInFileManager
        );
        assert_eq!(
            deliverable_activation_action(Path::new("missing.md"), false, false),
            DeliverableActivationAction::RevealInFileManager
        );
    }

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
            rotation: None,
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
            chains: Vec::new(),
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
            purpose: None,
            assignment_kind: Default::default(),
            plan_binding: None,
            plan_readiness: None,
            plan_blockers: Vec::new(),
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
            boss_authority: false,
            pinned_files: Vec::new(),
            expired: false,
            workspace_transition: false,
            expired_at: None,
            needs_attention: false,
            attention_viewed_at: None,
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
    fn employee_attention_unread_rearms_on_expiry_and_view_stamp() {
        let mut employee = boss_employee(None);
        employee.needs_attention = true;
        assert!(!employee_attention_is_unread(&employee));

        employee.expired = true;
        employee.expired_at = Some(100);
        assert!(employee_attention_is_unread(&employee));
        employee.attention_viewed_at = Some(99);
        assert!(employee_attention_is_unread(&employee));
        employee.attention_viewed_at = Some(100);
        assert!(!employee_attention_is_unread(&employee));

        // A fresh manual flag clears its prior view stamp, even when the
        // employee's old expiry has not changed.
        employee.attention_viewed_at = None;
        assert!(employee_attention_is_unread(&employee));
        employee.expired = false;
        assert!(!employee_attention_is_unread(&employee));
        employee.expired = true;
        employee.needs_attention = false;
        assert!(!employee_attention_is_unread(&employee));
    }

    #[test]
    fn employees_keep_active_order_and_sort_idle_by_latest_completion() {
        let mut queued = boss_employee(Some(300));
        queued.state = waku_protocol::boss::EmployeeLifecycle::Queued;
        let working = boss_employee(Some(200));

        let mut created_later = boss_employee(Some(900));
        created_later.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 950);
        let mut completed_later = boss_employee(Some(100));
        completed_later.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 950);
        let mut tie_first = boss_employee(Some(1));
        tie_first.session_id = Uuid::from_u128(1);
        tie_first.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 950);
        let mut tie_second = boss_employee(Some(2));
        tie_second.session_id = Uuid::from_u128(2);
        tie_second.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 950);
        let mut legacy_reply = boss_employee(Some(50));
        legacy_reply.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 950);
        let mut no_completion = boss_employee(Some(70));
        no_completion.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 950);

        let session_for = |id, created_at, last_reply_at, completion| {
            let mut session = waku_protocol::model::AgentSession::new(
                Uuid::new_v4(),
                waku_protocol::model::ProviderKind::Codex,
            );
            session.id = id;
            session.created_at = created_at;
            session.last_reply_at = last_reply_at;
            if let Some(completion) = completion {
                session.begin_turn("fixture");
                let turn = session.turns.last_mut().unwrap();
                turn.status = waku_protocol::model::TurnStatus::Completed;
                turn.completed_at = Some(completion);
            }
            session
        };
        let sessions = vec![
            session_for(created_later.session_id, 900, Some(100), Some(100)),
            session_for(completed_later.session_id, 100, Some(900), Some(900)),
            session_for(tie_first.session_id, 1, Some(500), Some(500)),
            session_for(tie_second.session_id, 2, Some(500), Some(500)),
            session_for(legacy_reply.session_id, 50, Some(800), None),
            session_for(no_completion.session_id, 70, None, None),
        ];
        let sessions_by_id = sessions
            .iter()
            .map(|session| (session.id, session))
            .collect::<HashMap<_, _>>();
        // Completed turns outrank creation and last_reply_at; legacy sessions
        // use last_reply_at, then the old summon/session-creation fallback.
        let mut employees = vec![
            &created_later,
            &no_completion,
            &working,
            &tie_second,
            &legacy_reply,
            &queued,
            &tie_first,
            &completed_later,
        ];
        sort_boss_employees(&mut employees, &sessions_by_id);
        assert_eq!(
            employees
                .iter()
                .map(|employee| employee.session_id)
                .collect::<Vec<_>>(),
            vec![
                queued.session_id,
                working.session_id,
                completed_later.session_id,
                legacy_reply.session_id,
                tie_first.session_id,
                tie_second.session_id,
                created_later.session_id,
                no_completion.session_id,
            ]
        );
    }

    #[test]
    fn idle_employees_without_completion_keep_legacy_creation_fallback() {
        let mut legacy = boss_employee(None);
        legacy.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 400);
        let mut stamped = boss_employee(Some(200));
        stamped.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 400);
        let mut unknown = boss_employee(None);
        unknown.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 400);
        let mut session = waku_protocol::model::AgentSession::new(
            Uuid::new_v4(),
            waku_protocol::model::ProviderKind::Codex,
        );
        session.id = legacy.session_id;
        session.created_at = 300;
        let sessions = HashMap::from([(session.id, &session)]);
        let mut employees = vec![&unknown, &stamped, &legacy];

        sort_boss_employees(&mut employees, &sessions);
        assert_eq!(
            employees
                .iter()
                .map(|employee| employee.session_id)
                .collect::<Vec<_>>(),
            vec![legacy.session_id, stamped.session_id, unknown.session_id]
        );
    }

    #[test]
    fn resumed_employees_rejoin_active_order_then_return_to_completion_order() {
        fn sorted_ids(
            finished: &waku_protocol::boss::BossEmployee,
            peer: &waku_protocol::boss::BossEmployee,
            session: &waku_protocol::model::AgentSession,
            peer_session: &waku_protocol::model::AgentSession,
        ) -> Vec<Uuid> {
            let sessions = HashMap::from([(session.id, session), (peer_session.id, peer_session)]);
            let mut employees = vec![finished, peer];
            sort_boss_employees(&mut employees, &sessions);
            employees
                .iter()
                .map(|employee| employee.session_id)
                .collect()
        }

        let mut finished = boss_employee(Some(100));
        finished.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 200);
        let mut peer = boss_employee(Some(200));
        peer.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 300);
        let mut session = waku_protocol::model::AgentSession::new(
            Uuid::new_v4(),
            waku_protocol::model::ProviderKind::Codex,
        );
        session.id = finished.session_id;
        session.begin_turn("first turn");
        session.turns.last_mut().unwrap().status = waku_protocol::model::TurnStatus::Completed;
        session.turns.last_mut().unwrap().completed_at = Some(900);
        let mut peer_session = waku_protocol::model::AgentSession::new(
            Uuid::new_v4(),
            waku_protocol::model::ProviderKind::Codex,
        );
        peer_session.id = peer.session_id;
        peer_session.begin_turn("peer turn");
        peer_session.turns.last_mut().unwrap().status = waku_protocol::model::TurnStatus::Completed;
        peer_session.turns.last_mut().unwrap().completed_at = Some(300);

        assert_eq!(
            sorted_ids(&finished, &peer, &session, &peer_session),
            vec![finished.session_id, peer.session_id]
        );

        // Resume makes the employee active; finishing a newer turn moves it
        // back to the idle head using the new turn's completion timestamp.
        finished.expired = false;
        finished.state = waku_protocol::boss::EmployeeLifecycle::Working;
        assert_eq!(
            sorted_ids(&finished, &peer, &session, &peer_session),
            vec![finished.session_id, peer.session_id]
        );
        finished.set_lifecycle(waku_protocol::boss::EmployeeLifecycle::Expired, 1_000);
        session.begin_turn("resumed turn");
        session.turns.last_mut().unwrap().completed_at = Some(1_100);
        assert_eq!(
            sorted_ids(&finished, &peer, &session, &peer_session),
            vec![finished.session_id, peer.session_id]
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
            AvatarStyle::Mascots,
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
            human_interaction: false,
            force_start: false,
            composer_receipts: Vec::new(),
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
            purpose: None,
            assignment_kind: Default::default(),
            plan_binding: None,
            plan_readiness: None,
            plan_blockers: Vec::new(),
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
            boss_authority: false,
            pinned_files: Vec::new(),
            expired: lifecycle == waku_protocol::boss::EmployeeLifecycle::Expired,
            workspace_transition: false,
            expired_at: None,
            needs_attention: false,
            attention_viewed_at: None,
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
            approved_revisions: Vec::new(),
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
            approved_revisions: Vec::new(),
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

    #[test]
    fn queue_position_counts_within_the_ticket_queue_root() {
        use waku_protocol::boss::EmployeeLifecycle;
        let mut state = boss_state_for_queue_test();
        // `employee()` puts every ticket under the same supervisor root.
        let head = employee(1, EmployeeLifecycle::Queued, Some(ticket(3, Vec::new())));
        let tail = employee(2, EmployeeLifecycle::Queued, Some(ticket(7, Vec::new())));
        // A queued ticket under another root is not a queue-mate.
        let mut foreign = employee(9, EmployeeLifecycle::Queued, Some(ticket(1, Vec::new())));
        foreign.supervisor_id = Uuid::from_u128(42);
        // A chain resolves to its top supervisor's root, matching the
        // daemon's `queue_root` walk.
        let mut subordinate = employee(4, EmployeeLifecycle::Queued, Some(ticket(5, Vec::new())));
        subordinate.supervisor_id = head.session_id;
        state.employees = vec![
            head.clone(),
            tail.clone(),
            foreign.clone(),
            subordinate.clone(),
        ];

        assert_eq!(boss_queue_position(&state, head.session_id), Some((1, 3)));
        assert_eq!(
            boss_queue_position(&state, subordinate.session_id),
            Some((2, 3))
        );
        assert_eq!(boss_queue_position(&state, tail.session_id), Some((3, 3)));
        // The foreign-root ticket is alone in its group.
        assert_eq!(
            boss_queue_position(&state, foreign.session_id),
            Some((1, 1))
        );
        // A dispatched ticket reports no position.
        let mut working = employee(6, EmployeeLifecycle::Working, None);
        working.supervisor_id = head.supervisor_id;
        state.employees.push(working.clone());
        assert_eq!(boss_queue_position(&state, working.session_id), None);
        // The settled ticket no longer counts as a queue-mate either.
        assert_eq!(boss_queue_position(&state, tail.session_id), Some((3, 3)));
    }

    #[test]
    fn queued_card_copy_tracks_the_ticket_state() {
        use waku_protocol::boss::{AdmissionBlocker, EmployeeLifecycle};

        // Waiting on a model slot names the wait; position calibrates when
        // the ticket is not at the head, else the slot count explains it.
        let limited = employee(
            1,
            EmployeeLifecycle::Queued,
            Some(ticket(
                1,
                vec![AdmissionBlocker::ModelLimit { used: 6, limit: 6 }],
            )),
        );
        let (phase, headline, secondary) = queued_card_copy(&limited, Some((1, 1)), false);
        assert_eq!(phase, QueuedCardPhase::Waiting);
        assert_eq!(headline, tr!("boss.queue_card_waiting_slot"));
        assert_eq!(
            secondary,
            Some(tr!(
                "boss.queue_card_slots_in_use",
                used = 6u32,
                limit = 6u32
            ))
        );
        let (_, _, secondary) = queued_card_copy(&limited, Some((2, 4)), false);
        assert_eq!(
            secondary,
            Some(tr!(
                "boss.queue_card_position",
                position = 2usize,
                total = 4usize
            ))
        );

        // A resource wait carries the broker's detail string through.
        let resources = employee(
            2,
            EmployeeLifecycle::Queued,
            Some(ticket(
                1,
                vec![AdmissionBlocker::HostResources {
                    detail: "waiting for a build slot".into(),
                }],
            )),
        );
        let (phase, headline, secondary) = queued_card_copy(&resources, Some((1, 2)), false);
        assert_eq!(phase, QueuedCardPhase::Waiting);
        assert_eq!(headline, tr!("boss.queue_card_waiting_resources"));
        assert_eq!(secondary.as_deref(), Some("waiting for a build slot"));

        // No blocker yet — the plain Queued headline, and a mid-queue seat
        // still reports its position.
        let plain = employee(3, EmployeeLifecycle::Queued, Some(ticket(1, Vec::new())));
        let (phase, headline, secondary) = queued_card_copy(&plain, None, false);
        assert_eq!(phase, QueuedCardPhase::Waiting);
        assert_eq!(headline, tr!("boss.goals_status_queued"));
        assert_eq!(secondary, None);
        let (_, _, secondary) = queued_card_copy(&plain, Some((2, 3)), false);
        assert_eq!(
            secondary,
            Some(tr!(
                "boss.queue_card_position",
                position = 2usize,
                total = 3usize
            ))
        );

        // A committed startNow — flagged on the ticket or still in flight —
        // reads Starting while admission has not reported a blocker.
        let mut forced = plain.clone();
        forced.ticket.as_mut().unwrap().force_start = true;
        let (phase, headline, secondary) = queued_card_copy(&forced, None, false);
        assert_eq!(phase, QueuedCardPhase::Starting);
        assert_eq!(headline, tr!("boss.goals_status_starting"));
        assert_eq!(secondary, None);
        let (phase, ..) = queued_card_copy(&plain, None, true);
        assert_eq!(phase, QueuedCardPhase::Starting);

        // A committed start the broker still cannot grant declines with the
        // truthful reason — the card re-arms rather than faking dispatch.
        let mut declined = limited.clone();
        declined.ticket.as_mut().unwrap().force_start = true;
        let (phase, headline, secondary) = queued_card_copy(&declined, None, false);
        assert_eq!(phase, QueuedCardPhase::Declined);
        assert_eq!(
            headline,
            tr!(
                "boss.queue_card_still_waiting",
                reason = tr!("boss.queue_card_slots_in_use", used = 6u32, limit = 6u32)
            )
        );
        assert_eq!(secondary, None);
    }

    #[test]
    fn the_queued_cards_button_sends_the_start_now_control() {
        // The card wires to the daemon's force-start control op — the wire
        // token is what the scheduler's single dispatch path recognizes.
        let action = serde_json::to_value(waku_protocol::boss::EmployeeControl::StartNow).unwrap();
        assert_eq!(action, serde_json::json!({ "type": "startNow" }));
    }

    fn boss_chat_session(project: Uuid, id: Uuid, archived: bool) -> AgentSession {
        let mut session = AgentSession::new(project, ProviderKind::Codex);
        session.id = id;
        session.boss_managed = true;
        session.archived_at = archived.then_some(1);
        session
    }

    fn resolve_composer_without_rotation_pushes(context: BossCommandContext) -> BossComposerSend {
        let identity = boss_state_for_queue_test().identity;
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        // Both app caches still describe the pre-rotation chat. The old
        // resolver cannot discover the replacement from this snapshot.
        let cached = vec![boss_chat_session(identity.id, old, false)];
        assert_eq!(
            live_boss_chat_session(&cached, &HashSet::from([identity.id]), &HashMap::new(), old),
            old
        );
        let mut active = boss_chat_session(identity.id, next, false);
        active.reasoning_effort = Some("high".into());
        active.pending_provider_context = Some("rotation handoff".into());
        let mut project = Project::from_path(std::env::temp_dir());
        project.id = identity.id;
        let mut submission = ComposerSubmission::plain("keep this draft".into());
        submission.display_content = Some("visible draft".into());
        submission.human_content = Some("user draft".into());
        let mut send = BossComposerSend {
            command: BossCommand {
                session_id: old,
                identity,
                context,
            },
            submission,
        };
        let mut requests = 0;
        let (resolved, _) = send
            .command
            .resolve(
                BossOperation::Open {
                    provider: ProviderKind::Codex,
                    model: None,
                    mode: Default::default(),
                },
                |command| {
                    requests += 1;
                    match command {
                        waku_client::Command::Boss {
                            operation: BossOperation::Open { .. },
                        } => {
                            assert_eq!(requests, 1);
                            Ok(waku_client::ResponsePayload::Boss {
                                result: BossResult::Session {
                                    session: Box::new(active.list_projection()),
                                    project: Box::new(project.clone()),
                                },
                            })
                        }
                        waku_client::Command::HydrateSession { session_id } => {
                            assert_eq!(requests, 2);
                            assert_eq!(session_id, next);
                            Ok(waku_client::ResponsePayload::Session {
                                session: Some(active.clone()),
                            })
                        }
                        _ => panic!("resolve before sending any runtime-scoped command"),
                    }
                },
            )
            .unwrap();
        assert_eq!(requests, 2);
        assert_eq!(send.command.session_id, next);
        assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(
            resolved.pending_provider_context.as_deref(),
            Some("rotation handoff")
        );
        assert_eq!(send.submission.prompt, "keep this draft");
        assert_eq!(
            send.submission.display_content.as_deref(),
            Some("visible draft")
        );
        assert_eq!(send.submission.human_content.as_deref(), Some("user draft"));
        send
    }

    #[test]
    fn employee_composer_rotation_without_pushes_resolves_active_chat_and_keeps_context() {
        let employee = Uuid::new_v4();
        let send = resolve_composer_without_rotation_pushes(BossCommandContext::Employee(employee));
        assert!(matches!(send.command.context, BossCommandContext::Employee(id) if id == employee));
    }

    #[test]
    fn deliverable_composer_rotation_without_pushes_resolves_active_chat_and_keeps_context() {
        let path = PathBuf::from("/boss/reports/summary.md");
        let send = resolve_composer_without_rotation_pushes(BossCommandContext::Deliverable {
            path: path.clone(),
            name: "Summary".into(),
            directory: false,
        });
        assert!(
            matches!(send.command.context, BossCommandContext::Deliverable {
            path: actual, name, directory: false,
        } if actual == path && name == "Summary")
        );
    }

    #[test]
    fn composer_rotation_lookup_failure_keeps_draft_and_armed_context() {
        let employee = Uuid::new_v4();
        let old = Uuid::new_v4();
        let mut send = BossComposerSend {
            command: BossCommand {
                session_id: old,
                identity: boss_state_for_queue_test().identity,
                context: BossCommandContext::Employee(employee),
            },
            submission: ComposerSubmission::plain("keep this draft".into()),
        };
        assert!(
            send.command
                .resolve(
                    BossOperation::Open {
                        provider: ProviderKind::Codex,
                        model: None,
                        mode: Default::default()
                    },
                    |_| anyhow::bail!("offline"),
                )
                .is_err()
        );
        assert_eq!(send.command.session_id, old);
        assert_eq!(send.submission.prompt, "keep this draft");
        assert!(matches!(send.command.context, BossCommandContext::Employee(id) if id == employee));
    }

    #[test]
    fn composer_rotation_rejects_a_chat_archived_during_lookup() {
        let identity = boss_state_for_queue_test().identity;
        let old = Uuid::new_v4();
        let next = boss_chat_session(identity.id, Uuid::new_v4(), false);
        let mut project = Project::from_path(std::env::temp_dir());
        project.id = identity.id;
        let mut command = BossCommand {
            session_id: old,
            identity,
            context: BossCommandContext::Employee(Uuid::new_v4()),
        };
        let result = command.resolve(
            BossOperation::Open {
                provider: ProviderKind::Codex,
                model: None,
                mode: Default::default(),
            },
            |request| match request {
                waku_client::Command::Boss { .. } => Ok(waku_client::ResponsePayload::Boss {
                    result: BossResult::Session {
                        session: Box::new(next.list_projection()),
                        project: Box::new(project.clone()),
                    },
                }),
                waku_client::Command::HydrateSession { .. } => {
                    let mut archived = next.clone();
                    archived.archived_at = Some(1);
                    Ok(waku_client::ResponsePayload::Session {
                        session: Some(archived),
                    })
                }
                _ => panic!("no runtime redirect"),
            },
        );
        assert!(result.is_err());
        assert_eq!(command.session_id, old);
    }

    #[test]
    fn employee_chat_submission_after_rotation_targets_the_live_chat() {
        // The employee-page composer armed while the document still named
        // the retired chat: the catalog already carries the swap, so the
        // send resolves to the replacement rather than landing on the
        // archived session the daemon refuses.
        let project = Uuid::new_v4();
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        let sessions = vec![
            boss_chat_session(project, old, true),
            boss_chat_session(project, next, false),
        ];
        let identities = HashSet::from([project]);
        assert_eq!(
            live_boss_chat_session(&sessions, &identities, &HashMap::new(), old),
            next
        );
        // The same resolution settles a draft parked on the retired chat —
        // a follow-up queued before the swap.
        assert_eq!(
            live_boss_chat_session(&sessions, &identities, &HashMap::from([(old, next)]), old),
            next
        );
        // With no replacement in the catalog yet the recorded chat stands —
        // the send proceeds on the id it armed with.
        let unswapped = vec![boss_chat_session(project, old, false)];
        assert_eq!(
            live_boss_chat_session(&unswapped, &identities, &HashMap::new(), old),
            old
        );
    }

    #[test]
    fn deliverable_submission_after_rotation_targets_the_live_chat() {
        // The deliverable page armed the command before the refresh landed:
        // the recorded swap resolves the retired chat to its replacement
        // even while the catalog still shows the old row unarchived.
        let project = Uuid::new_v4();
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        let sessions = vec![
            boss_chat_session(project, old, false),
            boss_chat_session(project, next, false),
        ];
        let identities = HashSet::from([project]);
        let rotated = HashMap::from([(old, next)]);
        assert_eq!(
            live_boss_chat_session(&sessions, &identities, &rotated, old),
            next
        );
        // Chained rotations follow to the live chat.
        let newer = Uuid::new_v4();
        let rotated = HashMap::from([(old, next), (next, newer)]);
        let chained = vec![
            boss_chat_session(project, old, true),
            boss_chat_session(project, next, true),
            boss_chat_session(project, newer, false),
        ];
        assert_eq!(
            live_boss_chat_session(&chained, &identities, &rotated, old),
            newer
        );
    }

    #[test]
    fn rotation_activation_lands_on_the_live_chat_not_the_archive() {
        // A visit, a Back hop, or a pending activation aimed at the retired
        // chat lands on its successor — the session swap is not a
        // navigation boundary.
        let project = Uuid::new_v4();
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        let sessions = vec![
            boss_chat_session(project, old, true),
            boss_chat_session(project, next, false),
        ];
        let identities = HashSet::from([project]);
        assert_eq!(
            boss_chat_activation_target(&sessions, &identities, &HashMap::new(), old),
            next
        );
        // A recorded swap alone cannot redirect — the successor's row must
        // be in the catalog before a landing may aim at it.
        let retired_only = vec![boss_chat_session(project, old, true)];
        assert_eq!(
            boss_chat_activation_target(
                &retired_only,
                &identities,
                &HashMap::from([(old, next)]),
                old
            ),
            old
        );
        // The live chat, unrelated sessions, and non-chat boss surfaces
        // keep their own target.
        assert_eq!(
            boss_chat_activation_target(&sessions, &identities, &HashMap::new(), next),
            next
        );
        let unrelated = Uuid::new_v4();
        assert_eq!(
            boss_chat_activation_target(&sessions, &identities, &HashMap::new(), unrelated),
            unrelated
        );
    }

    #[test]
    fn rotation_retargeting_never_touches_employee_sessions_or_live_chats() {
        let project = Uuid::new_v4();
        let employee_project = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let archived_employee = Uuid::new_v4();
        let sibling_employee = Uuid::new_v4();
        let sessions = vec![
            boss_chat_session(project, chat, false),
            boss_chat_session(employee_project, archived_employee, true),
            boss_chat_session(employee_project, sibling_employee, false),
        ];
        let identities = HashSet::from([project]);
        // An archived employee keeps its own id — its project is not a
        // boss workspace, so a live colleague in it is not a replacement.
        assert_eq!(
            live_boss_chat_session(
                &sessions,
                &identities,
                &HashMap::new(),
                archived_employee
            ),
            archived_employee
        );
        // A live recorded chat answers itself even with a stale rotation
        // map — the session that still takes prompts wins.
        assert_eq!(
            live_boss_chat_session(&sessions, &identities, &HashMap::new(), chat),
            chat
        );
        // Planning sessions are a different boss-owned kind; an archived
        // one never retargets onto the chat.
        let mut planning = boss_chat_session(project, Uuid::new_v4(), true);
        planning.planning = Some(crate::model::SessionPlanning {
            plan_file: "plans/auth.md".into(),
            idea: "Auth".into(),
            label: waku_client::WireTranslation::new("boss.planning_label", []),
            finalized_at: None,
        });
        let sessions = vec![planning, boss_chat_session(project, chat, false)];
        assert_eq!(
            live_boss_chat_session(
                &sessions,
                &identities,
                &HashMap::new(),
                sessions[0].id
            ),
            sessions[0].id
        );
    }

    #[test]
    fn pending_boss_open_activates_the_live_chat() {
        let project = Uuid::new_v4();
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        let identities = HashSet::from([project]);
        // A live pointer activates straight from the client's copy.
        let sessions = vec![boss_chat_session(project, old, false)];
        assert_eq!(
            pending_boss_open_session(&sessions, &identities, &HashMap::new(), Some(old))
                .map(|session| session.id),
            Some(old)
        );
        // A pointer still naming the retired chat activates its cataloged
        // replacement instead of landing on the archive.
        let sessions = vec![
            boss_chat_session(project, old, true),
            boss_chat_session(project, next, false),
        ];
        assert_eq!(
            pending_boss_open_session(&sessions, &identities, &HashMap::new(), Some(old))
                .map(|session| session.id),
            Some(next)
        );
    }

    #[test]
    fn pending_boss_open_parks_on_a_stale_pointer() {
        let project = Uuid::new_v4();
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        let identities = HashSet::from([project]);
        // The retired chat with no cataloged replacement is a stale
        // pointer — activating it would land the click on the archive.
        let sessions = vec![boss_chat_session(project, old, true)];
        assert!(
            pending_boss_open_session(&sessions, &identities, &HashMap::new(), Some(old))
                .is_none()
        );
        // A recorded swap alone cannot make the retired pointer safe: the
        // replacement's row must be in the catalog first.
        assert!(
            pending_boss_open_session(
                &sessions,
                &identities,
                &HashMap::from([(old, next)]),
                Some(old),
            )
            .is_none()
        );
        // A pointer naming a session the catalog has not received — and no
        // pointer at all — park the same way.
        assert!(
            pending_boss_open_session(&sessions, &identities, &HashMap::new(), Some(next))
                .is_none()
        );
        assert!(
            pending_boss_open_session(&sessions, &identities, &HashMap::new(), None).is_none()
        );
    }

    #[test]
    fn queued_boss_open_survives_a_rotation_repoint() {
        let project = Uuid::new_v4();
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        let other = Uuid::new_v4();
        let identities = HashSet::from([project]);
        let sessions = vec![
            boss_chat_session(project, old, true),
            boss_chat_session(project, next, false),
        ];
        // An unchanged selection re-issues, and so does a selection the
        // repoint moved onto the parked chat's live replacement.
        assert!(queued_boss_open_still_applies(
            &sessions,
            &identities,
            &HashMap::new(),
            Some(old),
            Some(old)
        ));
        assert!(queued_boss_open_still_applies(
            &sessions,
            &identities,
            &HashMap::new(),
            Some(old),
            Some(next)
        ));
        // The recorded swap carries the repoint across a catalog that still
        // shows the predecessor unarchived.
        let staged = vec![
            boss_chat_session(project, old, false),
            boss_chat_session(project, next, false),
        ];
        assert!(queued_boss_open_still_applies(
            &staged,
            &identities,
            &HashMap::from([(old, next)]),
            Some(old),
            Some(next)
        ));
        // A genuinely different destination is a departure — the parked
        // open must not drag the user back.
        assert!(!queued_boss_open_still_applies(
            &sessions,
            &identities,
            &HashMap::new(),
            Some(old),
            Some(other)
        ));
        assert!(!queued_boss_open_still_applies(
            &sessions,
            &identities,
            &HashMap::new(),
            None,
            Some(other)
        ));
        assert!(queued_boss_open_still_applies(
            &sessions,
            &identities,
            &HashMap::new(),
            None,
            None
        ));
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
            rotation: None,
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
            chains: Vec::new(),
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

    #[test]
    fn open_reply_loses_navigation_authority_to_a_picker_commit() {
        // ⌘N on the boss chat commits each destination by changing the
        // selected session; an Open reply already in flight must not undo
        // the landing. Before the gate existed, the reply activated the
        // chat unconditionally and snapped the user back.
        let chat = Uuid::new_v4();
        let project_draft = Uuid::new_v4();
        let projectless_draft = Uuid::new_v4();
        assert!(!boss_open_reply_navigates(
            Some(chat),
            Some(project_draft)
        ));
        assert!(!boss_open_reply_navigates(
            Some(chat),
            Some(projectless_draft)
        ));
        // A Boss pick keeps the same selection, so the open's intent still
        // stands — as does any open whose surface was never left.
        assert!(boss_open_reply_navigates(Some(chat), Some(chat)));
        // An open issued with nothing selected navigates only while nothing
        // was selected since.
        assert!(boss_open_reply_navigates(None, None));
        assert!(!boss_open_reply_navigates(None, Some(project_draft)));
    }

    /// The chip pick binds a session id, not a roster slot: reorders keep
    /// the same employee, an employee leaving the roster (retired, or one
    /// whose boss never sent one) reads stale so a send refuses closed
    /// rather than landing on the boss chat the draft was not written for,
    /// and a pick under one daemon never resolves under another.
    #[test]
    fn composer_target_pick_tracks_the_roster_by_session_id() {
        let key = DaemonKey::Local;
        let other_key = DaemonKey::Remote(Uuid::new_v4());
        let employee = Uuid::new_v4();
        let coworker = Uuid::new_v4();
        let mut ui = BossUi::default();
        // Nothing picked: the boss chat stays the destination.
        assert!(matches!(
            ui.composer_target_pick(key),
            ComposerTargetPick::Unpicked
        ));
        ui.composer_targets.insert(key, employee);
        // A missing roster cannot confirm the pick — stale, never live.
        assert!(matches!(
            ui.composer_target_pick(key),
            ComposerTargetPick::Stale(id) if id == employee
        ));
        ui.recent.insert(key, vec![employee, coworker]);
        assert!(matches!(
            ui.composer_target_pick(key),
            ComposerTargetPick::Live(id) if id == employee
        ));
        // Reordering the roster does not retarget the pick.
        ui.recent.insert(key, vec![coworker, employee]);
        assert!(matches!(
            ui.composer_target_pick(key),
            ComposerTargetPick::Live(id) if id == employee
        ));
        // Retirement drops the employee from the roster — the pick stays
        // armed against the session id but reads stale.
        ui.recent.insert(key, vec![coworker]);
        assert!(matches!(
            ui.composer_target_pick(key),
            ComposerTargetPick::Stale(id) if id == employee
        ));
        // The same session id picked under another daemon is inert here.
        assert!(matches!(
            ui.composer_target_pick(other_key),
            ComposerTargetPick::Unpicked
        ));
    }

    #[test]
    fn employee_composer_retry_keeps_identity_and_stale_replies_keep_newer_submissions() {
        let mut ui = BossUi::default();
        let employee = Uuid::new_v4();
        let delivery = waku_client::AgentPromptDelivery::Queue;
        let first = ui.employee_composer_submission(employee, "original", delivery);
        let repeated = ui.employee_composer_submission(employee, "original", delivery);
        assert_ne!(first, repeated); // distinct fresh sends, including in flight
        ui.restore_employee_composer_submission(employee, first, "original".into(), delivery);
        assert_eq!(first, ui.employee_composer_submission(employee, "original", delivery));
        ui.restore_employee_composer_submission(employee, first, "original".into(), delivery);
        let edited = ui.employee_composer_submission(employee, "edited", delivery);
        assert_ne!(first, edited);
        ui.restore_employee_composer_submission(employee, edited, "edited".into(), delivery);
        ui.accept_employee_composer_submission(employee, first);
        assert_eq!(edited, ui.employee_composer_submission(employee, "edited", delivery));
        ui.restore_employee_composer_submission(employee, edited, "edited".into(), delivery);
        ui.accept_employee_composer_submission(employee, edited);
        assert_ne!(edited, ui.employee_composer_submission(employee, "edited", delivery));
    }

    #[test]
    fn employee_composer_interaction_requires_the_current_chats_explicit_switch() {
        let viewed = Uuid::new_v4();
        let other = Uuid::new_v4();
        let context = BossCommandContext::Employee(viewed);
        assert!(employee_composer_interaction(&context, Some(viewed), Some(viewed), true));
        for (selected, target, switched) in [
            (Some(viewed), Some(viewed), false), // ordinary/global pick
            (Some(other), Some(viewed), true), // another chat
            (Some(viewed), Some(other), true), // another employee
            (Some(viewed), None, true), // switched back to Boss
            (None, Some(viewed), true),
        ] {
            assert!(!employee_composer_interaction(&context, selected, target, switched));
        }
        let deliverable = BossCommandContext::Deliverable {
            path: PathBuf::from("report.md"), name: "Report".into(), directory: false,
        };
        assert!(!employee_composer_interaction(&deliverable, Some(viewed), Some(viewed), true));
    }

    /// An employee chat's composer names only the boss and its own
    /// employee: a pick made for a colleague elsewhere resolves unpicked
    /// while the command answers to this page — the send commands the
    /// boss rather than landing on a recipient the menu cannot offer.
    /// The chat's own employee keeps its pick, live or stale, and other
    /// armed contexts keep the whole roster's picks.
    #[test]
    fn employee_chat_composer_target_scopes_the_pick_to_its_employee() {
        let chat = Uuid::new_v4();
        let coworker = Uuid::new_v4();
        let employee_context = BossCommandContext::Employee(chat);
        let deliverable_context = BossCommandContext::Deliverable {
            path: PathBuf::from("/boss/reports/summary.md"),
            name: "Summary".into(),
            directory: false,
        };
        // The chat's own employee keeps its pick — live or stale.
        assert!(matches!(
            composer_target_pick_for_context(&employee_context, ComposerTargetPick::Live(chat)),
            ComposerTargetPick::Live(id) if id == chat
        ));
        assert!(matches!(
            composer_target_pick_for_context(&employee_context, ComposerTargetPick::Stale(chat)),
            ComposerTargetPick::Stale(id) if id == chat
        ));
        // A colleague's pick — live or stale — reads unpicked on this page.
        assert!(matches!(
            composer_target_pick_for_context(
                &employee_context,
                ComposerTargetPick::Live(coworker)
            ),
            ComposerTargetPick::Unpicked
        ));
        assert!(matches!(
            composer_target_pick_for_context(
                &employee_context,
                ComposerTargetPick::Stale(coworker)
            ),
            ComposerTargetPick::Unpicked
        ));
        // Nothing picked stays unpicked.
        assert!(matches!(
            composer_target_pick_for_context(&employee_context, ComposerTargetPick::Unpicked),
            ComposerTargetPick::Unpicked
        ));
        // Other armed contexts keep the whole roster's picks.
        assert!(matches!(
            composer_target_pick_for_context(
                &deliverable_context,
                ComposerTargetPick::Live(coworker)
            ),
            ComposerTargetPick::Live(id) if id == coworker
        ));
        assert!(matches!(
            composer_target_pick_for_context(
                &deliverable_context,
                ComposerTargetPick::Stale(coworker)
            ),
            ComposerTargetPick::Stale(id) if id == coworker
        ));
    }

    /// The detail pane's instruction document lists each effective layer
    /// as a labeled bullet in the agent's composed order — inherited
    /// base, shipped default, appended customization — with the layer's
    /// text nested beneath the bullet.
    #[test]
    fn persona_layers_document_lists_layers_in_composed_order() {
        let layers = vec![
            PersonaInstructionLayer::Inherited {
                persona_id: Uuid::new_v4(),
                name: "Employee".into(),
                markdown: "Complete the bounded assignment.\n\nReport useful results.".into(),
            },
            PersonaInstructionLayer::Shipped {
                role: PersonaDefaultRole::Researcher,
                markdown: "Research only; report findings.".into(),
            },
            PersonaInstructionLayer::Custom {
                markdown: "Prefer primary sources.".into(),
            },
        ];
        let document = persona_layers_document(&layers);
        let inherited = document.find("- **Inherited — Employee**").unwrap();
        let shipped = document.find("- **Shipped — Researcher**").unwrap();
        let custom = document.find("- **Custom**").unwrap();
        assert!(inherited < shipped && shipped < custom);
        // Layer text nests beneath its bullet, paragraph breaks intact.
        assert!(
            document.contains(
                "- **Inherited — Employee**\n\n  Complete the bounded assignment.\n\n  Report useful results.\n"
            )
        );
        // An unmodified shipped role renders base + shipped only.
        let document = persona_layers_document(&layers[..2]);
        assert!(document.contains("- **Inherited — Employee**"));
        assert!(document.contains("- **Shipped — Researcher**"));
        assert!(!document.contains("**Custom**"));
    }
}
