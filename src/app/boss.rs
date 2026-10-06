//! Desktop control plane for daemon-owned Boss roles.
use super::boss_moods::AVATAR_SOURCE_SIZE;
use super::*;
use crate::ui::ActivationExt;
use waku_client::DaemonKey;
use waku_client::boss::{
    BossFile, BossIdentity, BossOperation, BossPersona, BossPersonaUpsert, BossResult, BossState,
    MemoryOperation, PersonaPermissions,
};
use waku_protocol::custom_commands::CustomCommandIcon;

/// The logical size a session-mention chip's avatar occupies in the
/// transcript — `ATOM_AVATAR_SCALE` of a body-text chip's height.
const MENTION_AVATAR_SIZE: f32 = 18.0;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BossTab {
    Memory,
    Personas,
    Employees,
    Plans,
    Deliverables,
}

/// The Memory section's two read-only views: the Boss's document buckets
/// under `memory/` in its files root, and the named bucket engine's
/// structured records. Documents and records stay distinct — records never
/// appear as files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BossMemoryView {
    Documents,
    Records,
}

/// A named memory bucket the Boss's files-canonical engine lists — parsed
/// off the `memory` operation's loose JSON so the app crate does not take a
/// `waku-memory-engine` dependency.
#[derive(Clone, Debug, Eq, PartialEq)]
struct MemoryBucket {
    id: String,
    name: String,
    purpose: String,
}

/// One readable record inside a named bucket's overview: an original note
/// or a stored chronological summary covering a note range.
#[derive(Clone, Debug, Eq, PartialEq)]
enum MemoryRecord {
    Note {
        sequence: u64,
        kind: waku_client::boss::MemoryNoteKind,
        text: String,
        created_at: u64,
    },
    Summary { start: u64, end: u64, text: String },
}

/// The overview item as the daemon serializes it — `{"type":"note"|"summary"}`.
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum MemoryOverviewItemWire {
    Note { note: MemoryNoteWire },
    Summary { summary: MemorySummaryWire },
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemoryNoteWire {
    sequence: u64,
    kind: waku_client::boss::MemoryNoteKind,
    text: String,
    created_at: u64,
}

#[derive(serde::Deserialize)]
struct MemorySummaryWire {
    start: u64,
    end: u64,
    text: String,
}

fn memory_bucket_from_json(value: &serde_json::Value) -> Option<MemoryBucket> {
    Some(MemoryBucket {
        id: value.get("id")?.as_str()?.to_owned(),
        name: value.get("name")?.as_str()?.to_owned(),
        purpose: value
            .get("purpose")
            .and_then(|purpose| purpose.as_str())
            .unwrap_or_default()
            .to_owned(),
    })
}

fn memory_record_from_json(value: &serde_json::Value) -> Option<MemoryRecord> {
    match serde_json::from_value::<MemoryOverviewItemWire>(value.clone()).ok()? {
        MemoryOverviewItemWire::Note { note } => Some(MemoryRecord::Note {
            sequence: note.sequence,
            kind: note.kind,
            text: note.text,
            created_at: note.created_at,
        }),
        MemoryOverviewItemWire::Summary { summary } => Some(MemoryRecord::Summary {
            start: summary.start,
            end: summary.end,
            text: summary.text,
        }),
    }
}

pub(super) struct BossUi {
    pub states: HashMap<DaemonKey, BossState>,
    pub(super) goal_rows: HashMap<DaemonKey, Arc<Vec<BossGoalRow>>>,
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
    pub projects: HashMap<Uuid, Project>,
    pub hosts: Vec<DaemonKey>,
    pub managed: HashSet<Uuid>,
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
    /// Memory entry armed as context for a user-requested correction in
    /// Boss chat — `(key, label, content)` where the label is the document
    /// path or a `buckets/<name>/note-<seq>` record reference.
    pub(super) command_memory_correction: Option<(DaemonKey, String, String)>,
    /// The deliverable whose row click is still navigating to its task page —
    /// the boss chat. Session activation clears `command_deliverable` as stale
    /// context, so the click parks its deliverable here and the finish reapplies
    /// the arm once the boss chat is on screen. The flag is whether the
    /// landing should write a history entry: a fresh row click pushes the
    /// page; a back/forward hop restoring it must not re-push.
    pub pending_deliverable: Option<(DaemonKey, Uuid, bool)>,
    /// The deliverable whose preview page covers the boss chat's transcript: a
    /// previewable file's own page rather than a panel off the chat. Lives
    /// and dies with `command_deliverable` — the same arm brands the composer's
    /// boss chip and roots the page's file at the deliverable's directory.
    pub deliverable_page: Option<(DaemonKey, Uuid)>,
    pub revision: u64,
    pub page: Option<(DaemonKey, BossTab)>,
    last_section: HashMap<DaemonKey, BossTab>,
    /// The Plans tab's selected plan — a `BossState.planning` record's
    /// session id, resolved against the live state at render.
    plans_selected: Option<Uuid>,
    /// Every loaded entry under the Boss files root's `memory/` tree —
    /// one flat list; folder rows parent their children by path prefix.
    files: Vec<BossFile>,
    files_key: Option<DaemonKey>,
    memory_expanded: HashMap<DaemonKey, HashSet<String>>,
    memory_loaded_folders: HashMap<DaemonKey, HashSet<String>>,
    memory_all_folders_loaded: HashSet<DaemonKey>,
    memory_loading_folder: Option<(DaemonKey, String)>,
    memory_error: Option<(DaemonKey, String, String)>,
    pub(super) memory_tree_width: f32,
    /// Read-only Boss memory document currently shown in the Brain reader.
    preview_file: Option<(DaemonKey, String, String)>,
    /// Documents or Records — the Memory section's active view per host.
    memory_view: HashMap<DaemonKey, BossMemoryView>,
    /// Named buckets the memory engine reports for a host — the Records
    /// view's collections.
    memory_buckets: HashMap<DaemonKey, Vec<MemoryBucket>>,
    memory_buckets_loading: HashSet<DaemonKey>,
    memory_buckets_error: HashMap<DaemonKey, String>,
    /// The Records view's selected bucket id per host.
    memory_bucket_selected: HashMap<DaemonKey, String>,
    /// Cached overview records per (host, bucket) — populated on select.
    memory_records: HashMap<(DaemonKey, String), Vec<MemoryRecord>>,
    memory_records_loading: Option<(DaemonKey, String)>,
    memory_records_error: HashMap<(DaemonKey, String), String>,
    editor: Option<BossEditor>,
    generation: u64,
    pending: bool,
    pending_reply: Option<BossReply>,
    list: ListState,
    scrollbar: Rc<ScrollbarState>,
    rows: Vec<BossItem>,
    avatar_queue: RefCell<VecDeque<(String, u32)>>,
    avatar_requested: RefCell<HashSet<(String, u32)>>,
    avatars: HashMap<String, HashMap<u32, Arc<gpui::RenderImage>>>,
    avatar_active: usize,
    focus: Option<FocusHandle>,
}

impl Default for BossUi {
    fn default() -> Self {
        Self {
            states: HashMap::new(),
            goal_rows: HashMap::new(),
            goals_finished_list: ListState::new(0, ListAlignment::Top, px(240.0)),
            goals_finished_scrollbar: ScrollbarState::new(),
            goals_ongoing_list: ListState::new(0, ListAlignment::Top, px(640.0)),
            goals_ongoing_scrollbar: ScrollbarState::new(),
            goals_list_owner: None,
            goals_finished_signature: None,
            goals_ongoing_signature: None,
            goals_collapsed: HashSet::new(),
            goals_history_expanded: HashSet::new(),
            projects: HashMap::new(),
            hosts: Vec::new(),
            managed: HashSet::new(),
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
            queued: HashMap::new(),
            dispatching: HashSet::new(),
            queued_model_targets: HashMap::new(),
            recent: HashMap::new(),
            sidebar_idle_visible: HashMap::new(),
            command_deliverable: None,
            command_memory_correction: None,
            pending_deliverable: None,
            deliverable_page: None,
            revision: 0,
            page: None,
            last_section: HashMap::new(),
            plans_selected: None,
            files: Vec::new(),
            files_key: None,
            memory_expanded: HashMap::new(),
            memory_loaded_folders: HashMap::new(),
            memory_all_folders_loaded: HashSet::new(),
            memory_loading_folder: None,
            memory_error: None,
            memory_tree_width: 280.0,
            preview_file: None,
            memory_view: HashMap::new(),
            memory_buckets: HashMap::new(),
            memory_buckets_loading: HashSet::new(),
            memory_buckets_error: HashMap::new(),
            memory_bucket_selected: HashMap::new(),
            memory_records: HashMap::new(),
            memory_records_loading: None,
            memory_records_error: HashMap::new(),
            editor: None,
            generation: 0,
            pending: false,
            pending_reply: None,
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

/// One employee goal prepared for the Goals panel. Rows carry the boss-owned
/// record; the render pass joins the cached session snapshot for status,
/// title, project, workspace, and update time.
#[derive(Clone)]
pub(super) struct BossGoalRow {
    pub session_id: Uuid,
    pub name: String,
    pub job_title: String,
    pub lifecycle: waku_protocol::boss::EmployeeLifecycle,
    /// The attention item the job reported — the row's small reason line.
    pub blocker: Option<String>,
    pub created_at: Option<u64>,
    pub expired_at: Option<u64>,
    pub queued_at: Option<u64>,
    /// The queued ticket's first nonempty prompt paragraph — present only
    /// while the assignment waits; dispatch clears the ticket's prompt.
    pub queued_objective: Option<String>,
    /// The project label the summon declared — the Pending row's project
    /// before its session shell exists.
    pub queued_project: Option<String>,
    /// The queued wait reason for the tooltip — the daemon's admission
    /// blocker or a neutral admission label, never a position number.
    pub queue_detail: Option<String>,
    /// 1-based admission order among the daemon's queued employees —
    /// Pending sorts on it; it is never displayed as a number.
    pub queue_rank: Option<usize>,
    /// Reserved wave grouping — retained in the prepared data, never
    /// displayed as a raw id.
    #[allow(dead_code)]
    pub group_id: Option<String>,
}


#[derive(Clone, Debug, Eq, PartialEq)]
enum BossItem {
    Employee(Uuid),
    Persona(Uuid),
    /// A `memory/` entry — `(path, directory, depth)`; directories at depth
    /// zero are the named buckets the tree tops out at.
    File(String, bool, usize),
    /// An expanded folder's in-place loading or failed-load status row.
    MemoryStatus(String, usize, bool),
    /// A named bucket's engine id — a Records view collection row.
    Bucket(String),
    /// A `BossState.planning` record's session — Plans tab rows.
    Plan(Uuid),
}

fn memory_tree_rows(
    files: &[BossFile],
    expanded: &HashSet<String>,
    loading: Option<&str>,
    error: Option<&str>,
) -> Vec<BossItem> {
    fn visit(
        parent: &str,
        depth: usize,
        files: &[BossFile],
        expanded: &HashSet<String>,
        loading: Option<&str>,
        error: Option<&str>,
        rows: &mut Vec<BossItem>,
    ) {
        let mut children = files
            .iter()
            .filter(|file| {
                file.path
                    .rsplit_once('/')
                    .map_or("", |(parent, _)| parent)
                    == parent
            })
            .collect::<Vec<_>>();
        children.sort_by(|a, b| {
            b.directory.cmp(&a.directory).then_with(|| {
                a.path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&a.path)
                    .to_lowercase()
                    .cmp(&b.path.rsplit('/').next().unwrap_or(&b.path).to_lowercase())
            })
        });
        for file in children {
            rows.push(BossItem::File(file.path.clone(), file.directory, depth));
            if file.directory && expanded.contains(&file.path) {
                if loading == Some(file.path.as_str()) {
                    rows.push(BossItem::MemoryStatus(file.path.clone(), depth + 1, false));
                } else if error == Some(file.path.as_str()) {
                    rows.push(BossItem::MemoryStatus(file.path.clone(), depth + 1, true));
                }
                visit(&file.path, depth + 1, files, expanded, loading, error, rows);
            }
        }
    }
    let mut rows = Vec::new();
    visit("memory", 0, files, expanded, loading, error, &mut rows);
    rows
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
    /// A read-only memory entry the user is asking the Boss to correct —
    /// `path` is the document's `memory/…` path or a record's
    /// `buckets/<name>/note-<seq>` label; `content` is the shown text.
    MemoryCorrection { path: String, content: String },
    Deliverable {
        path: PathBuf,
        name: String,
        directory: bool,
    },
}

struct BossEditor {
    key: DaemonKey,
    kind: BossEditorKind,
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
#[derive(Clone, Copy)]
enum BossEditorKind {
    Persona(Uuid),
    Name,
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum BossReply {
    Open,
    List,
    Read,
    /// A recursive `memory/` walk issued so filename search sees the whole
    /// tree — its result still lands as `BossResult::Files`.
    Search,
    /// `memory` bucket listing for the Records view.
    Buckets,
    /// A bucket's overview records for the Records view.
    Records,
    Saved,
    Finalize,
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
            || state.planning.iter().any(|plan| plan.session_id == session_id)
    })
}

impl Waku {
    pub(super) fn drain_boss_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok((key, state)) = self.boss_events.try_recv() {
            if self.boss_ui.states.get(&key).is_some_and(|previous| {
                previous.identity.id == state.identity.id && previous.revision > state.revision
            }) {
                continue;
            }
            let queue_rank = boss_queue_ranks(&state);
            let rows = Arc::new(
                state
                    .employees
                    .iter()
                    .filter(|employee| {
                        employee.work_goal == waku_protocol::boss::EmployeeGoal::Goal
                    })
                    .map(|employee| {
                        let queue_detail = boss_queue_detail(
                            employee,
                            queue_rank.get(&employee.session_id).copied(),
                        );
                        BossGoalRow {
                            session_id: employee.session_id,
                            name: employee.identity.name.clone(),
                            job_title: employee.job_title.clone(),
                            lifecycle: employee.lifecycle(),
                            blocker: employee.blocker.clone(),
                            created_at: employee.created_at,
                            expired_at: employee.expired_at,
                            queued_at: employee.queued_at,
                            queued_objective: employee
                                .ticket
                                .as_ref()
                                .and_then(|ticket| {
                                    ticket
                                        .prompt
                                        .split("\n\n")
                                        .map(str::trim)
                                        .find(|paragraph| !paragraph.is_empty())
                                        .map(str::to_owned)
                                }),
                            queued_project: employee
                                .ticket
                                .as_ref()
                                .map(|ticket| ticket.project.trim())
                                .filter(|project| !project.is_empty())
                                .map(str::to_owned),
                            queue_detail,
                            queue_rank: queue_rank.get(&employee.session_id).copied(),
                            group_id: employee
                                .ticket
                                .as_ref()
                                .and_then(|ticket| ticket.group_id.clone()),
                        }
                    })
                    .collect(),
            );
            self.boss_ui.goal_rows.insert(key, rows);
            // A plan document write bumps the Boss revision — re-arm every
            // planning session's read so a waiting strip mounts its plan tab
            // once the file holds real contents.
            let planning = state.planning.clone();
            self.boss_ui.states.insert(key, state);
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
                    if employee.expired {
                        self.boss_ui.expired.insert(employee.session_id);
                    } else {
                        self.boss_ui.working.insert(employee.session_id);
                    }
                    if let Some(detail) = boss_queue_detail(
                        employee,
                        queue_rank.get(&employee.session_id).copied(),
                    ) {
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
        self.pump_boss_avatars(cx);
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
        let Some(client) = self.daemons.supervisor(key).map(|supervisor| supervisor.client())
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
                    result:
                        BossResult::Memory {
                            buckets: raw, ..
                        },
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
        let Some(client) = self.daemons.supervisor(key).map(|supervisor| supervisor.client())
        else {
            self.boss_ui.memory_files_requested.borrow_mut().remove(&key);
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
                                    operation: BossOperation::ListFiles {
                                        path: path.clone(),
                                    },
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
                this.boss_ui.memory_files_requested.borrow_mut().remove(&key);
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

    fn sync_boss_page_rows(&mut self) {
        let Some((key, tab)) = self.boss_ui.page else {
            return;
        };
        let rows = match tab {
            BossTab::Memory => {
                match self
                    .boss_ui
                    .memory_view
                    .get(&key)
                    .copied()
                    .unwrap_or(BossMemoryView::Documents)
                {
                    BossMemoryView::Documents => {
                        let expanded = self
                            .boss_ui
                            .memory_expanded
                            .get(&key)
                            .cloned()
                            .unwrap_or_default();
                        let loading = self
                            .boss_ui
                            .memory_loading_folder
                            .as_ref()
                            .filter(|(loading_key, _)| *loading_key == key)
                            .map(|(_, path)| path.as_str());
                        let error = self
                            .boss_ui
                            .memory_error
                            .as_ref()
                            .filter(|(error_key, path, _)| {
                                *error_key == key && path != "memory"
                            })
                            .map(|(_, path, _)| path.as_str());
                        memory_tree_rows(&self.boss_ui.files, &expanded, loading, error)
                    }
                    BossMemoryView::Records => self
                        .boss_ui
                        .memory_buckets
                        .get(&key)
                        .into_iter()
                        .flatten()
                        .map(|bucket| BossItem::Bucket(bucket.id.clone()))
                        .collect(),
                }
            }
            BossTab::Employees => self
                .boss_ui
                .recent
                .get(&key)
                .into_iter()
                .flatten()
                .copied()
                .map(BossItem::Employee)
                .collect(),
            BossTab::Personas => self
                .boss_ui
                .states
                .get(&key)
                .into_iter()
                .flat_map(|state| &state.personas)
                .map(|persona| BossItem::Persona(persona.id))
                .collect(),
            // The page is the frozen-document archive — live drafts stay on
            // their planning session's sidebar row, not here.
            BossTab::Plans => self
                .boss_ui
                .states
                .get(&key)
                .into_iter()
                .flat_map(|state| &state.planning)
                .filter(|plan| plan.finalized_at.is_some())
                .map(|plan| BossItem::Plan(plan.session_id))
                .collect(),
            BossTab::Deliverables => Vec::new(),
        };
        if self.boss_ui.rows != rows {
            self.boss_ui.rows = rows;
            let row_height = if matches!(tab, BossTab::Memory)
                && self
                    .boss_ui
                    .memory_view
                    .get(&key)
                    .copied()
                    .unwrap_or(BossMemoryView::Documents)
                    == BossMemoryView::Documents
            {
                px(30.0)
            } else {
                px(42.0)
            };
            self.boss_ui
                .list
                .reset_with_uniform_height(self.boss_ui.rows.len(), row_height);
        }
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
        if tab == BossTab::Memory && self.boss_ui.files_key != Some(key) {
            self.boss_ui.files.clear();
            self.boss_ui.files_key = Some(key);
        }
        self.boss_ui.page = Some((key, tab));
        self.boss_ui.last_section.insert(key, tab);
        self.fold_terminals_group_for_navigation();
        self.sync_right_panel_owner(cx);
        self.sync_boss_page_rows();
        if tab == BossTab::Memory {
            match self
                .boss_ui
                .memory_view
                .get(&key)
                .copied()
                .unwrap_or(BossMemoryView::Documents)
            {
                BossMemoryView::Documents => self.boss_request(
                    key,
                    BossOperation::ListFiles {
                        path: "memory".into(),
                    },
                    BossReply::List,
                    cx,
                ),
                BossMemoryView::Records => self.ensure_boss_memory_buckets(key, cx),
            }
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
        if !self.state.boss_experiment_enabled || self.boss_ui.pending {
            return;
        }
        let list_path = match &operation {
            BossOperation::ListFiles { path } => Some(path.clone()),
            _ => None,
        };
        let memory_request_path = match &operation {
            BossOperation::ListFiles { path } | BossOperation::ReadFile { path } => {
                Some(path.clone())
            }
            _ => None,
        };
        let records_bucket = match &operation {
            BossOperation::Memory {
                operation:
                    waku_client::boss::MemoryOperation::Overview {
                        bucket: Some(bucket),
                        ..
                    },
            } if reply == BossReply::Records => Some(bucket.clone()),
            _ => None,
        };
        let Some(client) = self
            .daemons
            .supervisor(key)
            .map(|supervisor| supervisor.client())
        else {
            let unreachable = tr!("boss.unreachable").to_string();
            match reply {
                BossReply::List | BossReply::Read | BossReply::Search
                    if self
                        .boss_ui
                        .page
                        .is_some_and(|(page_key, tab)| page_key == key && tab == BossTab::Memory) =>
                {
                    self.boss_ui.memory_loading_folder = None;
                    self.boss_ui.memory_error = Some((
                        key,
                        memory_request_path.unwrap_or_else(|| "memory".into()),
                        unreachable,
                    ));
                    self.sync_boss_page_rows();
                }
                BossReply::Buckets => {
                    self.boss_ui.memory_buckets_loading.remove(&key);
                    self.boss_ui.memory_buckets_error.insert(key, unreachable);
                }
                BossReply::Records => {
                    self.boss_ui.memory_records_loading = None;
                    if let Some(bucket) = records_bucket {
                        self.boss_ui
                            .memory_records_error
                            .insert((key, bucket), unreachable);
                    }
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
        if let Some(path) = list_path.as_ref() {
            self.boss_ui.memory_error = None;
            self.boss_ui.memory_loading_folder = Some((key, path.clone()));
            self.sync_boss_page_rows();
        } else if memory_request_path.is_some() {
            self.boss_ui.memory_error = None;
        }
        if reply == BossReply::Buckets {
            self.boss_ui.memory_buckets_loading.insert(key);
            self.boss_ui.memory_buckets_error.remove(&key);
        }
        if let Some(bucket) = records_bucket.as_ref() {
            self.boss_ui.memory_records_loading = Some((key, bucket.clone()));
            self.boss_ui.memory_records_error.remove(&(key, bucket.clone()));
        }
        let tree_list_path = list_path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Filename search needs the whole tree: walk directories
                    // breadth-first and fold every listing into one `Files`.
                    if let Some(root) = tree_list_path
                        .as_deref()
                        .filter(|_| reply == BossReply::Search)
                    {
                        let mut directories = VecDeque::from([root.to_owned()]);
                        let mut all_files = Vec::new();
                        while let Some(path) = directories.pop_front() {
                            let response = client
                                .request(
                                    Uuid::nil(),
                                    Uuid::nil(),
                                    waku_client::Command::Boss {
                                        operation: BossOperation::ListFiles {
                                            path: path.clone(),
                                        },
                                    },
                                )
                                .map_err(|error| anyhow::anyhow!("{path}: {error}"))?;
                            let waku_client::ResponsePayload::Boss {
                                result: BossResult::Files { files },
                            } = response
                            else {
                                anyhow::bail!("unexpected memory directory response");
                            };
                            directories.extend(
                                files
                                    .iter()
                                    .filter(|file| file.directory)
                                    .map(|file| file.path.clone()),
                            );
                            all_files.extend(files);
                        }
                        Ok(waku_client::ResponsePayload::Boss {
                            result: BossResult::Files { files: all_files },
                        })
                    } else {
                        client.request(
                            Uuid::nil(),
                            Uuid::nil(),
                            waku_client::Command::Boss { operation },
                        )
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.boss_ui.generation != generation {
                    return;
                }
                this.boss_ui.pending = false;
                this.boss_ui.pending_reply = None;
                if this
                    .boss_ui
                    .memory_loading_folder
                    .as_ref()
                    .is_some_and(|(loading_key, _)| *loading_key == key)
                {
                    this.boss_ui.memory_loading_folder = None;
                }
                if reply == BossReply::Buckets {
                    this.boss_ui.memory_buckets_loading.remove(&key);
                }
                if reply == BossReply::Records {
                    this.boss_ui.memory_records_loading = None;
                }
                match result {
                    Ok(waku_client::ResponsePayload::Boss { result }) => {
                        match result {
                            BossResult::State { state } => {
                                if let (Some(editor), Some((values, _))) =
                                    (&mut this.boss_ui.editor, &saved)
                                {
                                    if matches!(editor.kind, BossEditorKind::Persona(id) if id.is_nil())
                                    {
                                        if let Some(persona) = state.personas.iter().rev().find(
                                            |persona| {
                                                persona.name == values[0].trim()
                                                    && persona.markdown == values[1]
                                            },
                                        ) {
                                            editor.kind = BossEditorKind::Persona(persona.id);
                                        }
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
                                if session.planning.is_none()
                                    && let Some(state) = this.boss_ui.states.get_mut(&key)
                                {
                                    state.session_id = Some(session.id);
                                }
                                let id = session.id;
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
                                if matches!(reply, BossReply::Finalize) =>
                            {
                                this.boss_request(key, BossOperation::View, BossReply::List, cx);
                            }
                            BossResult::Files { files } => {
                                if let Some((current_key, BossTab::Memory)) = this.boss_ui.page
                                    && current_key == key
                                    && let Some(path) = list_path.as_deref()
                                {
                                    this.boss_ui.memory_loading_folder = None;
                                    this.boss_ui.memory_error = None;
                                    if reply == BossReply::Search {
                                        this.boss_ui.files = files;
                                        this.boss_ui.memory_all_folders_loaded.insert(key);
                                        let loaded = this
                                            .boss_ui
                                            .memory_loaded_folders
                                            .entry(key)
                                            .or_default();
                                        loaded.insert("memory".into());
                                        loaded.extend(
                                            this.boss_ui
                                                .files
                                                .iter()
                                                .filter(|file| file.directory)
                                                .map(|file| file.path.clone()),
                                        );
                                    } else if path == "memory" {
                                        this.boss_ui.files = files;
                                        this.boss_ui.memory_loaded_folders.insert(
                                            key,
                                            HashSet::from(["memory".into()]),
                                        );
                                        this.boss_ui.memory_all_folders_loaded.remove(&key);
                                    } else {
                                        this.boss_ui.files.retain(|file| {
                                            file.path
                                                .rsplit_once('/')
                                                .map_or("", |(parent, _)| parent)
                                                != path
                                        });
                                        this.boss_ui.files.extend(files);
                                        this.boss_ui
                                            .memory_loaded_folders
                                            .entry(key)
                                            .or_default()
                                            .insert(path.to_owned());
                                    }
                                    this.boss_ui.files_key = Some(key);
                                    this.sync_boss_page_rows();
                                    if reply != BossReply::Search
                                        && !this
                                            .boss_memory_search
                                            .read(cx)
                                            .content()
                                            .trim()
                                            .is_empty()
                                    {
                                        this.ensure_boss_memory_search(cx);
                                    } else if reply == BossReply::List {
                                        this.ensure_boss_expanded_memory_folders(key, cx);
                                    }
                                }
                            }
                            BossResult::File { path, content }
                                if matches!(reply, BossReply::Read) =>
                            {
                                this.boss_ui.memory_error = None;
                                this.boss_ui.preview_file = Some((key, path, content));
                            }
                            BossResult::Memory {
                                buckets, overview, ..
                            } => match reply {
                                BossReply::Buckets => {
                                    this.boss_ui.memory_buckets_error.remove(&key);
                                    this.boss_ui.memory_buckets.insert(
                                        key,
                                        buckets
                                            .iter()
                                            .filter_map(memory_bucket_from_json)
                                            .collect(),
                                    );
                                    this.sync_boss_page_rows();
                                }
                                BossReply::Records => {
                                    if let Some(bucket) = records_bucket.as_ref() {
                                        let items = overview
                                            .as_ref()
                                            .and_then(|overview| overview.get("items"))
                                            .and_then(|items| items.as_array())
                                            .cloned()
                                            .unwrap_or_default();
                                        this.boss_ui.memory_records.insert(
                                            (key, bucket.clone()),
                                            items
                                                .iter()
                                                .filter_map(memory_record_from_json)
                                                .collect(),
                                        );
                                        this.boss_ui
                                            .memory_records_error
                                            .remove(&(key, bucket.clone()));
                                    }
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                        if matches!(reply, BossReply::Saved) {
                            if let (Some(editor), Some((values, permissions))) =
                                (&mut this.boss_ui.editor, saved)
                            {
                                editor.original = values;
                                editor.original_permissions = permissions;
                            }
                            if !this.boss_editor_dirty(cx) {
                                this.boss_ui.editor = None;
                            }
                            this.show_toast(tr!("boss.saved"));
                            this.boss_request(key, BossOperation::View, BossReply::List, cx);
                        }
                    }
                    Ok(_) => this.show_toast(tr!("boss.unexpected_response")),
                    Err(error) => {
                        let error = error.to_string();
                        if matches!(reply, BossReply::List | BossReply::Read | BossReply::Search)
                            && this
                                .boss_ui
                                .page
                                .is_some_and(|(page_key, tab)| {
                                    page_key == key && tab == BossTab::Memory
                                })
                        {
                            this.boss_ui.memory_loading_folder = None;
                            this.boss_ui.memory_error = Some((
                                key,
                                memory_request_path
                                    .clone()
                                    .unwrap_or_else(|| "memory".into()),
                                error,
                            ));
                            this.sync_boss_page_rows();
                        } else if reply == BossReply::Buckets {
                            this.boss_ui.memory_buckets_error.insert(key, error);
                        } else if reply == BossReply::Records {
                            if let Some(bucket) = records_bucket.as_ref() {
                                this.boss_ui
                                    .memory_records_error
                                    .insert((key, bucket.clone()), error);
                            }
                        } else {
                            this.show_toast(tr!("boss.failed", error = error.clone()));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Filename search reads every folder under `memory/` — kick off the
    /// recursive walk once while a query is active; it is a no-op once the
    /// whole tree is loaded.
    pub(super) fn ensure_boss_memory_search(&mut self, cx: &mut Context<Self>) {
        let Some((key, BossTab::Memory)) = self.boss_ui.page else {
            return;
        };
        if !self.boss_memory_search.read(cx).content().trim().is_empty()
            && !self.boss_ui.memory_all_folders_loaded.contains(&key)
            && !self.boss_ui.pending
        {
            self.boss_request(
                key,
                BossOperation::ListFiles {
                    path: "memory".into(),
                },
                BossReply::Search,
                cx,
            );
        }
    }

    /// Expanded folders load on demand: after each listing lands, fetch the
    /// next expanded folder that has not been read yet.
    fn ensure_boss_expanded_memory_folders(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
        if self.boss_ui.pending || self.boss_ui.memory_all_folders_loaded.contains(&key) {
            return;
        }
        let loaded = self.boss_ui.memory_loaded_folders.get(&key);
        let next = self
            .boss_ui
            .memory_expanded
            .get(&key)
            .into_iter()
            .flatten()
            .find(|path| {
                !loaded.is_some_and(|loaded| loaded.contains(*path))
                    && self
                        .boss_ui
                        .files
                        .iter()
                        .any(|file| file.directory && file.path.as_str() == path.as_str())
            })
            .cloned();
        if let Some(path) = next {
            self.boss_request(
                key,
                BossOperation::ListFiles { path },
                BossReply::List,
                cx,
            );
        }
    }

    /// The Records view's bucket list — loaded once per host on demand.
    fn ensure_boss_memory_buckets(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
        if self.boss_ui.memory_buckets.contains_key(&key)
            || self.boss_ui.memory_buckets_loading.contains(&key)
            || self.boss_ui.pending
        {
            return;
        }
        self.boss_request(
            key,
            BossOperation::Memory {
                operation: waku_client::boss::MemoryOperation::ListBuckets,
            },
            BossReply::Buckets,
            cx,
        );
    }

    /// A bucket's overview — loaded on first selection, cached per bucket.
    fn ensure_boss_memory_overview(
        &mut self,
        key: DaemonKey,
        bucket: String,
        cx: &mut Context<Self>,
    ) {
        if self.boss_ui.memory_records.contains_key(&(key, bucket.clone()))
            || self
                .boss_ui
                .memory_records_loading
                .as_ref()
                .is_some_and(|(loading_key, loading)| *loading_key == key && loading == &bucket)
            || self.boss_ui.pending
        {
            return;
        }
        self.boss_request(
            key,
            BossOperation::Memory {
                operation: waku_client::boss::MemoryOperation::Overview {
                    bucket: Some(bucket),
                    project: None,
                },
            },
            BossReply::Records,
            cx,
        );
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
        self.request_session_activation(command.session_id, SessionActivationTransition::Visit, cx);
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
            if let Some(image) =
                self.boss_avatar_image(&identity.avatar_seed, MENTION_AVATAR_SIZE)
            {
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
        let chats = self.boss_ui.states.values().filter_map(|state| {
            state.session_id.map(|id| (id, &state.identity))
        });
        let employees = self
            .boss_ui
            .identities
            .iter()
            .map(|(id, identity)| (*id, identity));
        Rc::new(
            chats
                .chain(employees)
                .filter_map(|(id, identity)| {
                    self.boss_avatar_image(&identity.avatar_seed, MENTION_AVATAR_SIZE)
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
        // Re-opening the boss chat hands the deliverable preview page back to
        // the chat transcript it covers, even when it was already selected —
        // the armed composer context and any parked landing die with it.
        self.boss_ui.command_deliverable = None;
        self.boss_ui.pending_deliverable = None;
        self.boss_ui.command_memory_correction = None;
        // The page's draft goes home before the chat's slot reclaims the
        // composer; without one the hint still needs the re-read.
        if !self.unmount_deliverable_page(cx) {
            self.sync_composer_placeholder(cx);
        }
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

    fn edit_boss_document(
        &mut self,
        key: DaemonKey,
        kind: BossEditorKind,
        name: String,
        content: String,
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
        let original = vec![
            name.clone(),
            content.clone(),
            pinned.clone(),
            buckets.clone(),
        ];
        let name = input(tr!("boss.name_path"), name, false, cx);
        let content = input(tr!("boss.markdown"), content, true, cx);
        let pinned = input(tr!("boss.pinned_files"), pinned, true, cx);
        let buckets = input("Granted memory bucket IDs".to_owned(), buckets, true, cx);
        let focus = name.read(cx).focus();
        self.boss_ui.editor = Some(BossEditor {
            key,
            kind,
            name,
            content,
            pinned,
            buckets,
            original,
            original_permissions: permissions.clone(),
            icon,
            original_icon: icon,
            integrations: if matches!(kind, BossEditorKind::Persona(_)) {
                self.daemons
                    .supervisor(key)
                    .map(|supervisor| {
                        supervisor
                            .settings()
                            .integrations
                            .iter()
                            .map(|setting| setting.id.clone())
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            },
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
        let operation = match editor.kind {
            BossEditorKind::Persona(id) => {
                let mut permissions = editor.permissions.clone();
                permissions.bucket_ids = lines(editor.buckets.read(cx).content());
                BossOperation::UpsertPersona {
                    persona: BossPersonaUpsert {
                        id,
                        name,
                        markdown: content,
                        pinned_files: lines(editor.pinned.read(cx).content()),
                        permissions,
                        // The editor tracks the icon itself — always
                        // replace rather than preserve.
                        icon: Some(editor.icon),
                    },
                }
            }
            BossEditorKind::Name => BossOperation::Rename { name },
        };
        self.boss_request(key, operation, BossReply::Saved, cx);
    }

    /// The raster cached for `(seed, size bucket)`, queueing a render when
    /// it is missing. Returns `None` while the raster is in flight so
    /// callers can draw their placeholder.
    pub(super) fn boss_avatar_image(
        &self,
        seed: &str,
        size: f32,
    ) -> Option<Arc<gpui::RenderImage>> {
        let bucket = avatar_bucket(size);
        if let Some(image) = self
            .boss_ui
            .avatars
            .get(seed)
            .and_then(|buckets| buckets.get(&bucket))
        {
            return Some(image.clone());
        }
        if self
            .boss_ui
            .avatar_requested
            .borrow_mut()
            .insert((seed.to_string(), bucket))
        {
            self.boss_ui
                .avatar_queue
                .borrow_mut()
                .push_back((seed.to_string(), bucket));
            signal_event_pump(&self.event_wake_tx);
        }
        None
    }

    pub(super) fn boss_avatar(&self, identity: &BossIdentity, size: f32, cx: &App) -> AnyElement {
        if let Some(image) = self.boss_avatar_image(&identity.avatar_seed, size) {
            return gpui::img(image)
                .size(px(size))
                .rounded(px(6.0))
                .into_any_element();
        }
        div()
            .size(px(size))
            .rounded(px(6.0))
            .bg(Theme::current(cx).overlay)
            .flex()
            .items_center()
            .justify_center()
            .child(identity.name.chars().next().unwrap_or('B').to_string())
            .into_any_element()
    }

    fn pump_boss_avatars(&mut self, cx: &mut Context<Self>) {
        while self.boss_ui.avatar_active < 4 {
            let Some((seed, bucket)) = self.boss_ui.avatar_queue.borrow_mut().pop_front() else {
                break;
            };
            self.boss_ui.avatar_active += 1;
            let renderer = cx.svg_renderer();
            let avatar_seed = seed.clone();
            cx.spawn(async move |this, cx| {
                let image = cx.background_executor().spawn(async move {
                    let svg = boss_moods::avatar_svg(&avatar_seed);
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        renderer.render_single_frame(&svg, avatar_scale(bucket))
                    }))
                    .ok()
                    .and_then(|result| result.ok())
                }).await;
                let _ = this.update(cx, |this, cx| {
                    this.boss_ui.avatar_active -= 1;
                    if let Some(image) = image {
                        let cached: usize = this
                            .boss_ui
                            .avatars
                            .values()
                            .map(|buckets| buckets.len())
                            .sum();
                        if cached >= 256 {
                            if let Some((old_seed, old_bucket)) = this
                                .boss_ui
                                .avatars
                                .iter()
                                .find_map(|(seed, buckets)| {
                                    buckets
                                        .keys()
                                        .next()
                                        .map(|bucket| (seed.clone(), *bucket))
                                })
                            {
                                if let Some(image) = this
                                    .boss_ui
                                    .avatars
                                    .get_mut(&old_seed)
                                    .and_then(|buckets| buckets.remove(&old_bucket))
                                {
                                    cx.drop_image(image, None);
                                }
                                if this
                                    .boss_ui
                                    .avatars
                                    .get(&old_seed)
                                    .is_some_and(|buckets| buckets.is_empty())
                                {
                                    this.boss_ui.avatars.remove(&old_seed);
                                }
                                this.boss_ui
                                    .avatar_requested
                                    .borrow_mut()
                                    .remove(&(old_seed, old_bucket));
                            }
                        }
                        this.boss_ui
                            .avatars
                            .entry(seed)
                            .or_default()
                            .insert(bucket, image);
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
            .child(self.boss_avatar(&state.identity, 24.0, cx))
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
                        let section = this.boss_ui.last_section.get(&key).copied().unwrap_or(BossTab::Memory);
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
                .id(SharedString::from(format!("boss-queued-{slot}-{session_id}")))
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
            .or_else(|| {
                session.and_then(|session| self.session_status_indicator(session, &theme))
            })
            .or_else(|| {
                self.boss_ui.dispatching.contains(&id).then(|| {
                    motion::spin_slow(icon(
                        "icons/loader-circle.svg",
                        12.0,
                        theme.text_secondary,
                    ))
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
                || (
                    session.map(|session| self.session_sidebar_model_detail(session)),
                    session.map(|session| session.provider),
                ),
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
            (self.boss_ui.job_titles.get(&id).cloned(), None)
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
            .child(self.boss_avatar(identity, 24.0, cx))
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
        let Some(deliverable) = self
            .boss_ui
            .states
            .get(&key)
            .and_then(|state| state.deliverables.iter().find(|deliverable| deliverable.id == deliverable_id))
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
        // The mini controls share the task row's chrome: zero-width until
        // the row is hovered or the button takes keyboard focus. The
        // Finder button is the escape hatch the in-app preview replaced —
        // remote deliverables keep it out since the path is not local.
        let finder_button = (key == DaemonKey::Local).then(|| {
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
            .id(SharedString::from(format!("deliverable-{key:?}-{deliverable_id}")))
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
            .tooltip(Tooltip::text(deliverable.path.clone()))
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
                            // The unread dot holds the row's right edge so
                            // the hover controls reveal to its left without
                            // moving it — the same glyph a task row's
                            // unseen-completion indicator draws.
                            .when(unread, |line| {
                                line.child(
                                    div()
                                        .flex_none()
                                        .size(px(12.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .size(px(7.0))
                                                .rounded_full()
                                                .bg(theme.info),
                                        ),
                                )
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
                                line.child(icon(
                                    "icons/pin-filled.svg",
                                    12.0,
                                    theme.text_tertiary,
                                ))
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
        div()
            .id(SharedString::from(id))
            .track_focus(focus)
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
            .group_hover(group_name.clone(), |style| {
                style.w(px(20.0)).opacity(1.0)
            })
            .focus_visible(|style| {
                style
                    .w(px(20.0))
                    .opacity(1.0)
                    .bg(theme.focus_highlight())
            })
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
        self.mark_deliverable_viewed(key, deliverable_id, cx);
        self.chat_with_boss(key, cx);
        self.boss_ui.command_deliverable = Some((key, deliverable_id));
        self.boss_ui.pending_deliverable = Some((key, deliverable_id, true));
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
        self.mark_deliverable_viewed(key, deliverable_id, cx);
        // Re-arming over a mounted page would orphan its draft slot — the
        // new arm stales `live_deliverable_page` while the composer still
        // holds the outgoing page's text.
        self.unmount_deliverable_page(cx);
        self.boss_ui.command_deliverable = Some((key, deliverable_id));
        self.boss_ui.pending_deliverable = Some((key, deliverable_id, false));
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
        let Some((key, deliverable_id, record_visit)) =
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
        self.restore_selected_composer_draft(cx);
        if record_visit {
            // The page mounts over the chat it parked on, so the chat is
            // what back returns to.
            self.session_navigation.visit(
                Some(NavigationLocation::Task(session_id)),
                NavigationLocation::Deliverable(key, deliverable_id),
            );
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

    pub(super) fn render_boss_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some((key, tab)) = self.boss_ui.page else {
            return div().into_any_element();
        };
        let focus = self
            .boss_ui
            .focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let theme = Theme::current(cx);
        let name = self
            .boss_ui
            .states
            .get(&key)
            .map(|state| state.identity.name.clone())
            .unwrap_or_else(|| tr!("boss.group"));
        let host = match key {
            DaemonKey::Local => tr!("boss.local_host"),
            DaemonKey::Remote(host) => self.remote_host_name(host).unwrap_or_else(|| tr!("boss.remote_host")),
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .p(px(16.0))
            .border_b_1()
            .border_color(theme.border)
            .child(div().flex_1().flex().flex_col()
                .child(div().text_size(sp(16.0)).child(name.clone()))
                .child(div().text_size(sp(11.0)).text_color(theme.text_tertiary).child(host)))
            .child(boss_button("boss-identity", tr!("boss.identity"), &theme)
                .child(tr!("boss.identity"))
                .on_activation(cx, move |this, window, cx| this.edit_boss_document(
                    key, BossEditorKind::Name, name.clone(), String::new(), None, window, cx)))
            .child(
                boss_button("boss-chat", tr!("boss.chat"), &theme)
                    .child(icon("icons/message-square.svg", 14.0, theme.text_secondary))
                    .child(tr!("boss.chat"))
                    .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx)),
            )
            ;
        let section_strip = div().flex().items_center().gap(px(2.0)).px(px(16.0)).py(px(6.0))
            .border_b_1().border_color(theme.separator)
            .children([
                (BossTab::Memory, "boss.memory", "icons/brain.svg"),
                (BossTab::Personas, "boss.personas", "icons/user-round.svg"),
                (BossTab::Employees, "boss.history", "icons/folder-clock.svg"),
                (BossTab::Plans, "boss.plans", "icons/map.svg"),
                (BossTab::Deliverables, "boss.deliverables", "icons/file-text.svg"),
            ].into_iter().map(|(target, label, glyph)| {
                boss_button(label, tr!(label), &theme)
                    .when(tab == target, |button| button.bg(theme.overlay))
                    .child(icon(glyph, 14.0, theme.text_secondary))
                    .child(tr!(label))
                    .on_activation(cx, move |this, window, cx| this.open_boss_page(key, target, window, cx))
            }));
        let memory_view = self
            .boss_ui
            .memory_view
            .get(&key)
            .copied()
            .unwrap_or(BossMemoryView::Documents);
        let mut toolbar = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(20.0))
            .py(px(10.0));
        match tab {
            BossTab::Memory => {
                let mut segment_chip = |view: BossMemoryView, label: String, id: &'static str| {
                    let active = memory_view == view;
                    div()
                        .id(id)
                        .h(px(24.0))
                        .px(px(9.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .text_size(sp(12.0))
                        .text_color(if active {
                            theme.text
                        } else {
                            theme.text_tertiary
                        })
                        .when(active, |element| element.bg(theme.overlay))
                        .cursor_pointer()
                        .tab_index(0)
                        .focus_visible(|element| element.bg(theme.focus_highlight()))
                        .hover(|element| element.bg(theme.overlay))
                        .child(label)
                        .on_activation(cx, move |this, _, cx| {
                            this.boss_ui.memory_view.insert(key, view);
                            this.sync_boss_page_rows();
                            if view == BossMemoryView::Records {
                                this.ensure_boss_memory_buckets(key, cx);
                            }
                            cx.notify();
                        })
                };
                toolbar = toolbar
                    .child(
                        div()
                            .h(px(26.0))
                            .px(px(3.0))
                            .rounded(px(7.0))
                            .bg(theme.sidebar_item_background)
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .child(segment_chip(
                                BossMemoryView::Documents,
                                tr!("boss.memory_documents"),
                                "boss-memory-documents",
                            ))
                            .child(segment_chip(
                                BossMemoryView::Records,
                                tr!("boss.memory_records"),
                                "boss-memory-records",
                            )),
                    )
                    .when(memory_view == BossMemoryView::Documents, |bar| {
                        bar.child(
                            TextField::new("boss-memory-search", self.boss_memory_search.clone())
                                .icon("icons/search.svg", 13.0)
                                .w(px(190.0)),
                        )
                    })
                    .child(div().flex_1());
            }
            BossTab::Employees => {
                toolbar = toolbar
                    .child(
                        div()
                            .flex_1()
                            .text_color(theme.text_secondary)
                            .child(tr!("boss.history_hint")),
                    );
            }
            BossTab::Plans => {
                toolbar = toolbar.child(
                    div()
                        .flex_1()
                        .text_color(theme.text_secondary)
                        .child(tr!("boss.plans_hint")),
                );
            }
            BossTab::Personas => {
                toolbar = toolbar
                    .child(
                        div()
                            .flex_1()
                            .text_color(theme.text_secondary)
                            .child(tr!("boss.persona_hint")),
                    )
                    .child(
                        boss_button("boss-new-persona", tr!("boss.new_persona"), &theme)
                            .child(tr!("boss.new_persona"))
                            .on_activation(cx, move |this, window, cx| {
                                this.edit_boss_document(
                                    key,
                                    BossEditorKind::Persona(Uuid::nil()),
                                    String::new(),
                                    String::new(),
                                    None,
                                    window,
                                    cx,
                                )
                            }),
                    );
            }
            BossTab::Deliverables => {
                toolbar = toolbar.child(div().flex_1().text_color(theme.text_secondary).child(tr!("boss.phase_later")));
            }
        }
        let memory_documents = tab == BossTab::Memory && memory_view == BossMemoryView::Documents;
        let query = if memory_documents {
            self.boss_memory_search
                .read(cx)
                .content()
                .trim()
                .to_lowercase()
        } else {
            String::new()
        };
        let visible_rows: Vec<BossItem> = if query.is_empty() {
            self.boss_ui.rows.clone()
        } else {
            // Search results flatten the tree — each row shows its full
            // `memory/…` path so the match's bucket stays visible.
            self.boss_ui
                .files
                .iter()
                .filter(|file| {
                    !file.directory && file.path.to_lowercase().contains(&query)
                })
                .map(|file| BossItem::File(file.path.clone(), false, 0))
                .collect()
        };
        let has_search_matches = query.is_empty() || !visible_rows.is_empty();
        self.boss_ui.list.reset_with_uniform_height(
            visible_rows.len(),
            px(if memory_documents { 30.0 } else { 42.0 }),
        );
        let weak = cx.entity().downgrade();
        let list = div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                list(self.boss_ui.list.clone(), move |visible_index, _, cx| {
                    let Some(item) = visible_rows.get(visible_index).cloned() else { return div().into_any_element(); };
                    weak.upgrade()
                        .map(|entity| {
                            entity.update(cx, |this, cx| this.render_boss_item(item, cx))
                        })
                        .unwrap_or_else(|| div().into_any_element())
                })
                .size_full(),
            )
            .child(scrollbar::vertical(
                &self.boss_ui.list,
                &self.boss_ui.scrollbar,
            ));
        let editor = self.render_boss_editor(cx);
        div()
            .track_focus(&focus)
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(header)
            .child(section_strip)
            .child(toolbar.border_b_1().border_color(theme.separator))
            .when(
                tab != BossTab::Memory && boss_loading_label_visible(self.boss_ui.pending),
                |element| element.child(div().px(px(20.0)).child(tr!("boss.loading"))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .w(px(if tab == BossTab::Memory {
                                self.boss_ui.memory_tree_width
                            } else {
                                300.0
                            }))
                            .flex()
                            .flex_col()
                            .min_h_0()
                            .relative()
                            .border_r_1()
                            .border_color(theme.separator)
                            .child(if memory_documents {
                                if let Some((_, _, error)) =
                                    self.boss_ui.memory_error.as_ref().filter(
                                        |(error_key, path, _)| {
                                            *error_key == key && path == "memory"
                                        },
                                    )
                                {
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .text_color(theme.text_secondary)
                                                .child(error.clone()),
                                        )
                                        .child(
                                            boss_button(
                                                "boss-memory-tree-retry",
                                                tr!("boss.retry"),
                                                &theme,
                                            )
                                            .child(tr!("boss.retry"))
                                            .on_activation(cx, move |this, _, cx| {
                                                this.boss_request(
                                                    key,
                                                    BossOperation::ListFiles {
                                                        path: "memory".into(),
                                                    },
                                                    BossReply::List,
                                                    cx,
                                                );
                                            }),
                                        )
                                } else if self.boss_ui.pending
                                    && self.boss_ui.pending_reply == Some(BossReply::Search)
                                {
                                    div()
                                        .flex_1()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(theme.text_secondary)
                                        .child(tr!("boss.loading"))
                                } else if self.boss_ui.files.is_empty()
                                    && self.boss_ui.pending
                                    && self.boss_ui.pending_reply == Some(BossReply::List)
                                {
                                    div()
                                        .flex_1()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(theme.text_secondary)
                                        .child(tr!("boss.loading"))
                                } else if !has_search_matches {
                                    div()
                                        .flex_1()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(theme.text_tertiary)
                                        .child(tr!("boss.search_no_matches"))
                                } else {
                                    list
                                }
                            } else if tab == BossTab::Memory {
                                if let Some(error) = self.boss_ui.memory_buckets_error.get(&key) {
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .text_color(theme.text_secondary)
                                                .child(error.clone()),
                                        )
                                        .child(
                                            boss_button(
                                                "boss-memory-buckets-retry",
                                                tr!("boss.retry"),
                                                &theme,
                                            )
                                            .child(tr!("boss.retry"))
                                            .on_activation(cx, move |this, _, cx| {
                                                this.boss_ui.memory_buckets.remove(&key);
                                                this.ensure_boss_memory_buckets(key, cx);
                                            }),
                                        )
                                } else if self.boss_ui.memory_buckets_loading.contains(&key) {
                                    div()
                                        .flex_1()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(theme.text_secondary)
                                        .child(tr!("boss.loading"))
                                } else {
                                    list
                                }
                            } else {
                                list
                            })
                            .when(tab == BossTab::Memory, |pane| {
                                pane.child(self.render_panel_resize_handle(
                                    "boss-memory-tree-resize",
                                    PanelResizeTarget::BossMemoryTree,
                                    cx,
                                ))
                            }),
                    )
                    .child(if tab == BossTab::Plans {
                        self.render_boss_plan_detail(key, cx)
                    } else if tab == BossTab::Memory {
                        match memory_view {
                            BossMemoryView::Documents => self.render_boss_memory_detail(key, cx),
                            BossMemoryView::Records => self.render_boss_records_detail(key, cx),
                        }
                    } else if tab == BossTab::Deliverables {
                        div().flex_1().flex().items_center().justify_center().text_color(theme.text_secondary).child(tr!("boss.phase_later")).into_any_element()
                    } else {
                        div()
                            .flex_1()
                            .min_w_0()
                            .id("boss-editor-scroll")
                            .overflow_y_scroll()
                            .p(px(20.0))
                            .child(editor)
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    fn render_boss_item(&self, item: BossItem, cx: &mut Context<Self>) -> AnyElement {
        let Some((key, _)) = self.boss_ui.page else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        match item {
            BossItem::Employee(id) => self.render_boss_employee_row(id, cx),
            BossItem::MemoryStatus(path, depth, failed) => {
                let label = if failed {
                    self.boss_ui
                        .memory_error
                        .as_ref()
                        .map(|(_, _, error)| error.clone())
                        .unwrap_or_else(|| {
                            tr!("boss.memory_unavailable", path = path.clone()).to_string()
                        })
                } else {
                    tr!("boss.loading").to_string()
                };
                if failed {
                    boss_button(format!("boss-memory-folder-retry-{path}"), label.clone(), &theme)
                        .h(px(30.0))
                        .w_full()
                        .pl(px(8.0 + depth as f32 * 16.0))
                        .child(icon("icons/rotate-cw.svg", 13.0, theme.text_secondary))
                        .child(div().truncate().child(label))
                        .on_activation(cx, move |this, _, cx| {
                            this.boss_request(
                                key,
                                BossOperation::ListFiles {
                                    path: path.clone(),
                                },
                                BossReply::List,
                                cx,
                            );
                        })
                        .into_any_element()
                } else {
                    div()
                        .h(px(30.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .pl(px(8.0 + depth as f32 * 16.0))
                        .text_size(sp(11.0))
                        .text_color(theme.text_tertiary)
                        .child(icon("icons/loader-circle.svg", 13.0, theme.text_tertiary))
                        .child(label)
                        .into_any_element()
                }
            }
            BossItem::File(path, directory, depth) => {
                let selected = self.boss_ui.preview_file.as_ref().is_some_and(
                    |(file_key, selected_path, _)| *file_key == key && selected_path == &path,
                );
                let searching = !self
                    .boss_memory_search
                    .read(cx)
                    .content()
                    .trim()
                    .is_empty();
                let display = if searching {
                    path.clone()
                } else {
                    path.rsplit('/').next().unwrap_or(&path).to_owned()
                };
                boss_button(format!("boss-file-{path}"), path.clone(), &theme)
                    .h(px(30.0))
                    .w_full()
                    .pl(px(8.0 + depth as f32 * 16.0))
                    .when(selected, |button| button.bg(theme.sidebar_item_background))
                    .child(icon(
                        // Depth-zero directories are the named buckets — the
                        // tree never shows a raw `memory` root row.
                        if directory {
                            if self
                                .boss_ui
                                .memory_expanded
                                .get(&key)
                                .is_some_and(|expanded| expanded.contains(&path))
                            {
                                "icons/chevron-down.svg"
                            } else {
                                "icons/chevron-right.svg"
                            }
                        } else {
                            "icons/file.svg"
                        },
                        13.0,
                        theme.text_secondary,
                    ))
                    .child(div().truncate().child(display))
                    .on_activation(cx, move |this, _, cx| {
                        if directory {
                            let expanded = this.boss_ui.memory_expanded.entry(key).or_default();
                            let opening = !expanded.remove(&path);
                            if opening {
                                expanded.insert(path.clone());
                            }
                            this.sync_boss_page_rows();
                            if opening
                                && !this
                                    .boss_ui
                                    .memory_loaded_folders
                                    .get(&key)
                                    .is_some_and(|loaded| loaded.contains(&path))
                            {
                                this.boss_request(
                                    key,
                                    BossOperation::ListFiles {
                                        path: path.clone(),
                                    },
                                    BossReply::List,
                                    cx,
                                );
                            }
                            cx.notify();
                        } else {
                            if searching {
                                let expanded =
                                    this.boss_ui.memory_expanded.entry(key).or_default();
                                let mut parent =
                                    path.rsplit_once('/').map(|(parent, _)| parent);
                                while let Some(directory) =
                                    parent.filter(|directory| *directory != "memory")
                                {
                                    expanded.insert(directory.to_owned());
                                    parent =
                                        directory.rsplit_once('/').map(|(parent, _)| parent);
                                }
                                this.sync_boss_page_rows();
                            }
                            this.boss_request(
                                key,
                                BossOperation::ReadFile {
                                    path: path.clone(),
                                },
                                BossReply::Read,
                                cx,
                            );
                        }
                    })
                    .into_any_element()
            }
            BossItem::Bucket(bucket_id) => {
                let Some(bucket) = self
                    .boss_ui
                    .memory_buckets
                    .get(&key)
                    .and_then(|buckets| buckets.iter().find(|bucket| bucket.id == bucket_id))
                    .cloned()
                else {
                    return div().into_any_element();
                };
                let selected = self
                    .boss_ui
                    .memory_bucket_selected
                    .get(&key)
                    .is_some_and(|selected| *selected == bucket_id);
                boss_button(
                    format!("boss-bucket-{bucket_id}"),
                    bucket.name.clone(),
                    &theme,
                )
                .h(px(42.0))
                .w_full()
                .when(selected, |button| button.bg(theme.overlay))
                .child(icon("icons/database.svg", 15.0, theme.text_secondary))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(div().truncate().child(bucket.name.clone()))
                        .when(!bucket.purpose.is_empty(), |element| {
                            element.child(
                                div()
                                    .text_size(sp(11.0))
                                    .text_color(theme.text_tertiary)
                                    .truncate()
                                    .child(bucket.purpose.clone()),
                            )
                        }),
                )
                .on_activation(cx, move |this, _, cx| {
                    this.boss_ui
                        .memory_bucket_selected
                        .insert(key, bucket_id.clone());
                    this.ensure_boss_memory_overview(key, bucket_id.clone(), cx);
                    cx.notify();
                })
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
                let selected = self.boss_ui.plans_selected == Some(session_id);
                let detail = plan.finalized_at.map(|finalized_at| {
                    tr!(
                        "boss.plan_finalized_ago",
                        ago = sidebar::format_time_ago(unix_time().saturating_sub(finalized_at))
                    )
                });
                boss_button(format!("boss-plan-{session_id}"), plan.idea.clone(), &theme)
                    .h(px(42.0))
                    .w_full()
                    .when(selected, |button| button.bg(theme.overlay))
                    .child(icon("icons/file-text.svg", 16.0, theme.text_secondary))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(div().truncate().child(plan.idea.clone()))
                            .when_some(detail, |element, detail| {
                                element.child(
                                    div()
                                        .text_size(sp(11.0))
                                        .text_color(theme.text_tertiary)
                                        .truncate()
                                        .child(detail),
                                )
                            }),
                    )
                    .on_activation(cx, move |this, _, cx| {
                        this.boss_ui.plans_selected = Some(session_id);
                        cx.notify();
                    })
                    .into_any_element()
            }
            BossItem::Persona(id) => {
                let Some(persona) = self
                    .boss_ui
                    .states
                    .get(&key)
                    .and_then(|state| state.personas.iter().find(|persona| persona.id == id))
                    .cloned()
                else {
                    return div().into_any_element();
                };
                boss_button(format!("persona-{id}"), persona.name.clone(), &theme)
                    .h(px(42.0))
                    .w_full()
                    .child(persona.name.clone())
                    .on_activation(cx, move |this, window, cx| {
                        this.edit_boss_document(
                            key,
                            BossEditorKind::Persona(id),
                            persona.name.clone(),
                            persona.markdown.clone(),
                            Some(persona.clone()),
                            window,
                            cx,
                        )
                    })
                    .into_any_element()
            }

        }
    }

    /// The Plans tab's reading pane — the selected frozen document through
    /// the same boss-file read the session's plan tab uses.
    fn render_boss_plan_detail(&mut self, key: DaemonKey, cx: &mut Context<Self>) -> AnyElement {
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
        let Some(session_id) = self.boss_ui.plans_selected else {
            return empty(tr!("boss.plan_select"));
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
        self.ensure_plan_doc(key, session_id, &plan.plan_file, false, cx);
        match self
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
        }
    }

    /// The Memory section's reading pane for documents: the selected file's
    /// rendered Markdown under a quiet path header, with the correction
    /// request that arms the boss chat's composer. Failures stay in this
    /// pane — the tree keeps its expansion and selection.
    fn render_boss_memory_detail(&mut self, key: DaemonKey, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        if let Some((error_key, path, error)) = self.boss_ui.memory_error.as_ref()
            && *error_key == key
            && path == "memory"
        {
            let error = error.clone();
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(
                    div().text_color(theme.text_secondary).child(format!(
                        "{}\n{error}",
                        tr!("boss.memory_unavailable", path = "memory")
                    )),
                )
                .child(
                    boss_button("boss-memory-preview-list-retry", tr!("boss.retry"), &theme)
                        .child(tr!("boss.retry"))
                        .on_activation(cx, move |this, _, cx| {
                            this.boss_request(
                                key,
                                BossOperation::ListFiles {
                                    path: "memory".into(),
                                },
                                BossReply::List,
                                cx,
                            );
                        }),
                )
                .into_any_element();
        }
        if let Some((error_key, path, error)) = self.boss_ui.memory_error.as_ref()
            && *error_key == key
            && path != "memory"
            && !self
                .boss_ui
                .files
                .iter()
                .any(|file| file.path == path.as_str() && file.directory)
        {
            let path = path.clone();
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(
                    div().text_color(theme.text_secondary).child(format!(
                        "{}\n{error}",
                        tr!("boss.memory_unavailable", path = path.clone())
                    )),
                )
                .child(
                    boss_button("boss-memory-preview-retry", tr!("boss.retry"), &theme)
                        .child(tr!("boss.retry"))
                        .on_activation(cx, move |this, _, cx| {
                            this.boss_request(
                                key,
                                BossOperation::ReadFile {
                                    path: path.clone(),
                                },
                                BossReply::Read,
                                cx,
                            );
                        }),
                )
                .into_any_element();
        }
        if self.boss_ui.pending && self.boss_ui.pending_reply == Some(BossReply::Read) {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.text_secondary)
                .child(tr!("boss.loading"))
                .into_any_element();
        }
        if let Some((file_key, path, content)) = self.boss_ui.preview_file.as_ref()
            && *file_key == key
        {
            let path = path.clone();
            let content = content.clone();
            let identity = self
                .boss_ui
                .states
                .get(&key)
                .map(|state| state.identity.id)
                .unwrap_or_default();
            let correct_path = path.clone();
            let correct_content = content.clone();
            return div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(38.0))
                        .px(px(18.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .border_b_1()
                        .border_color(theme.separator)
                        .child(icon("icons/file-text.svg", 14.0, theme.text_tertiary))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(theme.text_secondary)
                                .child(path),
                        )
                        .child(
                            boss_button(
                                "boss-memory-correction",
                                tr!("boss.ask_correct"),
                                &theme,
                            )
                            .child(tr!("boss.ask_correct"))
                            .on_activation(cx, move |this, _, cx| {
                                this.chat_with_boss(key, cx);
                                this.boss_ui.command_memory_correction =
                                    Some((key, correct_path.clone(), correct_content.clone()));
                                this.sync_composer_placeholder(cx);
                            }),
                        ),
                )
                .child(
                    div()
                        .id("boss-memory-preview")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px(px(28.0))
                        .py(px(20.0))
                        .child(self.plan_document_view(identity, &content, false, None, cx)),
                )
                .into_any_element();
        }
        if self.boss_ui.files.is_empty() && !self.boss_ui.pending {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(
                    div()
                        .text_color(theme.text_secondary)
                        .child(tr!("boss.memory_empty")),
                )
                .child(
                    boss_button("boss-memory-remember", tr!("boss.ask_remember"), &theme)
                        .child(tr!("boss.ask_remember"))
                        .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx)),
                )
                .into_any_element();
        }
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.text_secondary)
            .child(tr!("boss.memory_select"))
            .into_any_element()
    }

    /// The Records view's detail: the selected named bucket's overview —
    /// original notes and stored summaries as readable entries, with the
    /// same correction-through-Boss flow documents carry. Read-only.
    fn render_boss_records_detail(&mut self, key: DaemonKey, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let center = |label: String| {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .child(div().text_color(theme.text_secondary).child(label))
                .into_any_element()
        };
        let Some(bucket_id) = self.boss_ui.memory_bucket_selected.get(&key).cloned() else {
            return center(tr!("boss.bucket_select"));
        };
        let Some(bucket) = self
            .boss_ui
            .memory_buckets
            .get(&key)
            .and_then(|buckets| buckets.iter().find(|bucket| bucket.id == bucket_id))
            .cloned()
        else {
            return center(tr!("boss.bucket_select"));
        };
        self.ensure_boss_memory_overview(key, bucket_id.clone(), cx);
        if let Some(error) = self
            .boss_ui
            .memory_records_error
            .get(&(key, bucket_id.clone()))
        {
            let error = error.clone();
            let retry_bucket = bucket_id.clone();
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(
                    div().text_color(theme.text_secondary).child(format!(
                        "{}\n{error}",
                        tr!("boss.bucket_unavailable", bucket = bucket.name.clone())
                    )),
                )
                .child(
                    boss_button("boss-memory-records-retry", tr!("boss.retry"), &theme)
                        .child(tr!("boss.retry"))
                        .on_activation(cx, move |this, _, cx| {
                            this.boss_request(
                                key,
                                BossOperation::Memory {
                                    operation: waku_client::boss::MemoryOperation::Overview {
                                        bucket: Some(retry_bucket.clone()),
                                        project: None,
                                    },
                                },
                                BossReply::Records,
                                cx,
                            );
                        }),
                )
                .into_any_element();
        }
        let Some(records) = self
            .boss_ui
            .memory_records
            .get(&(key, bucket_id.clone()))
            .cloned()
        else {
            return center(tr!("boss.loading"));
        };
        if records.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(
                    div()
                        .text_color(theme.text_secondary)
                        .child(tr!("boss.records_empty")),
                )
                .child(
                    boss_button("boss-records-remember", tr!("boss.ask_remember"), &theme)
                        .child(tr!("boss.ask_remember"))
                        .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx)),
                )
                .into_any_element();
        }
        let bucket_name = bucket.name.clone();
        let rows = records
            .iter()
            .enumerate()
            .map(|(index, record)| {
                let (label, reference, text) = match record {
                    MemoryRecord::Note {
                        sequence,
                        kind,
                        text,
                        ..
                    } => {
                        let kind_label = match kind {
                            waku_client::boss::MemoryNoteKind::Fact => tr!("boss.record_fact"),
                            waku_client::boss::MemoryNoteKind::Observation => {
                                tr!("boss.record_observation")
                            }
                            waku_client::boss::MemoryNoteKind::Question => {
                                tr!("boss.record_question")
                            }
                        };
                        (
                            format!("#{sequence} · {kind_label}"),
                            format!("note-{sequence}"),
                            text.clone(),
                        )
                    }
                    MemoryRecord::Summary { start, end, text } => (
                        format!("#{}–{} · {}", start, end, tr!("boss.record_summary")),
                        format!("summary-{start}-{end}"),
                        text.clone(),
                    ),
                };
                let correct_label = format!("buckets/{}/{reference}", bucket.name);
                let correct_content = text.clone();
                div()
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(theme.separator)
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(sp(11.0))
                                    .text_color(theme.text_tertiary)
                                    .child(label),
                            )
                            .child(
                                boss_button(
                                    format!("boss-record-correct-{index}"),
                                    tr!("boss.ask_correct"),
                                    &theme,
                                )
                                .child(tr!("boss.ask_correct"))
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
                    .child(div().text_size(sp(13.0)).child(text))
            })
            .collect::<Vec<_>>();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(38.0))
                    .px(px(18.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .border_b_1()
                    .border_color(theme.separator)
                    .child(icon("icons/database.svg", 14.0, theme.text_tertiary))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(theme.text_secondary)
                            .child(format!("buckets/{bucket_name}")),
                    ),
            )
            .child(
                div()
                    .id("boss-memory-records")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(28.0))
                    .py(px(12.0))
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_boss_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(editor) = &self.boss_ui.editor else {
            return div()
                .text_color(theme.text_secondary)
                .child(tr!("boss.select_document"))
                .into_any_element();
        };
        let editor_tab = match editor.kind {
            BossEditorKind::Persona(_) => BossTab::Personas,
            BossEditorKind::Name => BossTab::Employees,
        };
        if self.boss_ui.page != Some((editor.key, editor_tab)) {
            return div()
                .text_color(theme.text_secondary)
                .child(tr!("boss.select_document"))
                .into_any_element();
        }
        let is_persona = matches!(editor.kind, BossEditorKind::Persona(_));
        let mut form = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(tr!("boss.name_path"))
            .child(boss_input(editor.name.clone(), &theme));
        if matches!(editor.kind, BossEditorKind::Persona(_)) {
            form = form
                .child(tr!("boss.markdown"))
                .child(boss_input(editor.content.clone(), &theme));
        }
        if is_persona {
            form = form
                .child("Granted memory bucket IDs")
                .child(boss_input(editor.buckets.clone(), &theme))
                .child(tr!("boss.pinned_files"))
                .child(boss_input(editor.pinned.clone(), &theme))
                .child("Persona icon (employees inherit this unless overridden)")
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
            {
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
            }
        }
        form.child(
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
        )
        .into_any_element()
    }
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
        .filter(|employee| {
            employee.lifecycle() == waku_protocol::boss::EmployeeLifecycle::Queued
        })
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
        Some(waku_protocol::boss::AdmissionBlocker::HostResources { detail }) => detail.clone(),
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
    (
        &["bug", "fix", "bugfix", "debug"],
        "icons/bug.svg",
    ),
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

    #[test]
    fn memory_tree_flattens_expanded_folders_and_hides_collapsed_children() {
        let files = vec![
            BossFile {
                path: "memory/z.md".into(),
                directory: false,
            },
            BossFile {
                path: "memory/work".into(),
                directory: true,
            },
            BossFile {
                path: "memory/work/note.md".into(),
                directory: false,
            },
        ];
        assert_eq!(
            memory_tree_rows(&files, &HashSet::new(), None, None),
            vec![
                BossItem::File("memory/work".into(), true, 0),
                BossItem::File("memory/z.md".into(), false, 0),
            ]
        );
        let expanded = HashSet::from(["memory/work".to_string()]);
        assert_eq!(
            memory_tree_rows(&files, &expanded, None, None),
            vec![
                BossItem::File("memory/work".into(), true, 0),
                BossItem::File("memory/work/note.md".into(), false, 1),
                BossItem::File("memory/z.md".into(), false, 0),
            ]
        );
        assert_eq!(
            memory_tree_rows(&files, &expanded, Some("memory/work"), None),
            vec![
                BossItem::File("memory/work".into(), true, 0),
                BossItem::MemoryStatus("memory/work".into(), 1, false),
                BossItem::File("memory/work/note.md".into(), false, 1),
                BossItem::File("memory/z.md".into(), false, 0),
            ]
        );
    }

    #[test]
    fn memory_overview_items_parse_notes_and_summaries() {
        let note = serde_json::json!({
            "type": "note",
            "note": {
                "id": "1-abc",
                "bucketId": "project-x",
                "sequence": 3,
                "kind": "observation",
                "text": "watch the queue",
                "retryKey": "k",
                "createdAt": 99
            }
        });
        let summary = serde_json::json!({
            "type": "summary",
            "summary": { "bucketId": "project-x", "start": 1, "end": 2, "text": "early notes" }
        });
        assert_eq!(
            memory_record_from_json(&note),
            Some(MemoryRecord::Note {
                sequence: 3,
                kind: waku_client::boss::MemoryNoteKind::Observation,
                text: "watch the queue".into(),
                created_at: 99,
            })
        );
        assert_eq!(
            memory_record_from_json(&summary),
            Some(MemoryRecord::Summary {
                start: 1,
                end: 2,
                text: "early notes".into(),
            })
        );
        assert_eq!(memory_record_from_json(&serde_json::json!({"type": "other"})), None);
    }

    fn boss_employee(created_at: Option<u64>) -> waku_protocol::boss::BossEmployee {
        waku_protocol::boss::BossEmployee {
            session_id: Uuid::new_v4(),
            supervisor_id: Uuid::new_v4(),
            identity: BossIdentity {
                id: Uuid::new_v4(),
                name: String::new(),
                avatar_seed: String::new(),
            },
            job_title: String::new(),
            persona_id: Uuid::new_v4(),
            work_goal: waku_protocol::boss::EmployeeGoal::Errand,
            created_at,
            icon: None,
            permissions: PersonaPermissions::default(),
            pinned_files: Vec::new(),
            expired: false,
            expired_at: None,
            blocker: None,
            cancelled: false,
            state: waku_protocol::boss::EmployeeLifecycle::Working,
            ticket: None,
            queued_at: None,
            request_id: None,
            request_fingerprint: None,
        }
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
            goal_id: None,
            reservation: None,
            pending_resources: None,
            pending_reservation: None,
            blocked_by,
            dispatch_event: None,
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
            },
            job_title: "Tester".into(),
            persona_id: Uuid::from_u128(u128::MAX - 1),
            work_goal: waku_protocol::boss::EmployeeGoal::Errand,
            created_at: None,
            icon: None,
            permissions: PersonaPermissions::default(),
            pinned_files: Vec::new(),
            expired: lifecycle == waku_protocol::boss::EmployeeLifecycle::Expired,
            expired_at: None,
            blocker: None,
            cancelled: false,
            state: lifecycle,
            ticket,
            queued_at: (lifecycle == waku_protocol::boss::EmployeeLifecycle::Queued).then_some(1),
            request_id: None,
            request_fingerprint: None,
        }
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
            session_id: planning,
            plan_file: "plans/auth.md".into(),
            idea: "Auth".into(),
            finalized_at: None,
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
    fn queue_ranks_follow_ticket_sequence() {
        let mut state = boss_state_for_queue_test();
        let head = employee(1, waku_protocol::boss::EmployeeLifecycle::Queued, Some(ticket(7, Vec::new())));
        let tail = employee(2, waku_protocol::boss::EmployeeLifecycle::Queued, Some(ticket(3, Vec::new())));
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
            Some(ticket(1, vec![AdmissionBlocker::ModelLimit { used: 6, limit: 6 }])),
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
            },
            persona_id: Uuid::from_u128(u128::MAX - 3),
            session_id: None,
            personas: Vec::new(),
            employees: Vec::new(),
            retired_employees: Vec::new(),
            deliverables: Vec::new(),
            goals_viewed_at: None,
            planning: Vec::new(),
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

}
