//! Desktop control plane for daemon-owned Boss roles.
use super::boss_moods::AVATAR_SOURCE_SIZE;
use super::*;
use crate::ui::ActivationExt;
use waku_client::DaemonKey;
use waku_client::boss::{
    BossFile, BossIdentity, BossOperation, BossPersona, BossResult, BossState, PersonaPermissions,
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
    History,
    Memory,
    Plans,
    Personas,
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
    pub(super) job_titles: HashMap<Uuid, String>,
    pub(super) employee_icons: HashMap<Uuid, Option<CustomCommandIcon>>,
    pub active: HashMap<DaemonKey, Vec<Uuid>>,
    pub working: HashSet<Uuid>,
    pub expired: HashSet<Uuid>,
    pub recent: HashMap<DaemonKey, Vec<Uuid>>,
    pub sidebar_idle_visible: HashMap<DaemonKey, usize>,
    /// The deliverable a sidebar click armed the composer with: the next main-
    /// composer submission commands the boss with this file attached.
    /// Cleared by a send or a session selection.
    pub command_deliverable: Option<(DaemonKey, Uuid)>,
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
    /// The Plans tab's selected plan — a `BossState.planning` record's
    /// session id, resolved against the live state at render.
    plans_selected: Option<Uuid>,
    files: Vec<BossFile>,
    files_key: Option<DaemonKey>,
    folder: String,
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
    loaded_file: Option<(DaemonKey, String, String)>,
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
            job_titles: HashMap::new(),
            employee_icons: HashMap::new(),
            active: HashMap::new(),
            working: HashSet::new(),
            expired: HashSet::new(),
            recent: HashMap::new(),
            sidebar_idle_visible: HashMap::new(),
            command_deliverable: None,
            pending_deliverable: None,
            deliverable_page: None,
            revision: 0,
            page: None,
            plans_selected: None,
            files: Vec::new(),
            files_key: None,
            folder: "memory".into(),
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
            loaded_file: None,
            focus: None,
        }
    }
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
    File(String, bool),
    /// A `BossState.planning` record's session — Plans tab rows.
    Plan(Uuid),
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
    memory: Entity<TextInput>,
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
    File,
    Folder,
    Name,
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum BossReply {
    Open,
    Finalize,
    List,
    Read,
    Saved,
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
            // Queue rank among every queued employee — errands included, so
            // a goal's neutral wait detail accounts for the hidden work
            // ahead of it without revealing a position number.
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
            let queue_rank: HashMap<Uuid, usize> = queued
                .iter()
                .enumerate()
                .map(|(index, employee)| (employee.session_id, index + 1))
                .collect();
            let rows = Arc::new(
                state
                    .employees
                    .iter()
                    .filter(|employee| {
                        employee.work_goal == waku_protocol::boss::EmployeeGoal::Goal
                    })
                    .map(|employee| {
                        let queue_detail = (employee.lifecycle()
                            == waku_protocol::boss::EmployeeLifecycle::Queued)
                            .then(|| {
                                match employee
                                    .ticket
                                    .as_ref()
                                    .and_then(|ticket| ticket.blocked_by.first())
                                {
                                    Some(
                                        waku_protocol::boss::AdmissionBlocker::ModelLimit {
                                            used,
                                            limit,
                                        },
                                    ) => tr!(
                                        "boss.goals_queue_model",
                                        model = employee
                                            .ticket
                                            .as_ref()
                                            .map(|ticket| ticket.model.as_str())
                                            .unwrap_or_default(),
                                        used = used,
                                        limit = limit
                                    ),
                                    Some(
                                        waku_protocol::boss::AdmissionBlocker::HostResources {
                                            detail,
                                        },
                                    ) => detail.clone(),
                                    None => {
                                        if queue_rank.get(&employee.session_id)
                                            == Some(&1)
                                        {
                                            tr!("boss.goals_queue_admission")
                                        } else {
                                            tr!("boss.goals_queue_earlier")
                                        }
                                    }
                                }
                            });
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
            self.boss_ui.states.insert(key, state);
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
            let timestamps: HashMap<Uuid, u64> = self
                .state
                .sessions
                .iter()
                .map(|session| (session.id, session.updated_at))
                .collect();
            for (key, state) in &self.boss_ui.states {
                if let Some(id) = state.session_id {
                    self.boss_ui.managed.insert(id);
                }
                let mut employees = state.employees.iter().collect::<Vec<_>>();
                employees.sort_by_key(|employee| {
                    std::cmp::Reverse(timestamps.get(&employee.session_id).copied().unwrap_or(0))
                });
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
                    self.boss_ui.employee_icons.insert(
                        employee.session_id,
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
                            }),
                    );
                    self.boss_ui.managed.insert(employee.session_id);
                    if employee.expired {
                        self.boss_ui.expired.insert(employee.session_id);
                    } else {
                        self.boss_ui.working.insert(employee.session_id);
                    }
                    self.boss_ui
                        .identities
                        .insert(employee.session_id, employee.identity.clone());
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
            BossTab::History => self
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
            BossTab::Memory => self
                .boss_ui
                .files
                .iter()
                .map(|file| BossItem::File(file.path.clone(), file.directory))
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
        };
        if self.boss_ui.rows != rows {
            self.boss_ui.rows = rows;
            self.boss_ui
                .list
                .reset_with_uniform_height(self.boss_ui.rows.len(), px(42.0));
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
            self.boss_ui.folder = "memory".into();
            self.boss_ui.files_key = Some(key);
        }
        self.boss_ui.page = Some((key, tab));
        self.fold_terminals_group_for_navigation();
        self.sync_right_panel_owner(cx);
        self.sync_boss_page_rows();
        if tab == BossTab::Memory {
            self.boss_request(
                key,
                BossOperation::ListFiles {
                    path: self.boss_ui.folder.clone(),
                },
                BossReply::List,
                cx,
            );
        }
        let focus = self
            .boss_ui
            .focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// A boss operation is in flight — `boss_request` drops additional asks
    /// until it settles, so callers that show a spinner or dim a button read
    /// this rather than tracking the request themselves.
    pub(super) fn boss_pending(&self) -> bool {
        self.boss_ui.pending
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
            return;
        }
        let Some(client) = self
            .daemons
            .supervisor(key)
            .map(|supervisor| supervisor.client())
        else {
            self.show_toast(tr!("boss.unreachable"));
            cx.notify();
            return;
        };
        let list_path = match &operation {
            BossOperation::ListFiles { path } => Some(path.clone()),
            _ => None,
        };
        let saved = matches!(reply, BossReply::Saved)
            .then(|| {
                self.boss_ui.editor.as_ref().map(|editor| {
                    (
                        [
                            &editor.name,
                            &editor.content,
                            &editor.pinned,
                            &editor.memory,
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
                    return;
                }
                this.boss_ui.pending = false;
                this.boss_ui.pending_reply = None;
                match result {
                    Ok(waku_client::ResponsePayload::Boss { result }) => {
                        match result {
                            BossResult::State { state } => {
                                if let (Some(editor), Some((values, _))) = (&mut this.boss_ui.editor, &saved) {
                                    if matches!(editor.kind, BossEditorKind::Persona(id) if id.is_nil()) {
                                        if let Some(persona) = state.personas.iter().rev().find(|persona| persona.name == values[0].trim() && persona.markdown == values[1]) {
                                            editor.kind = BossEditorKind::Persona(persona.id);
                                        }
                                    }
                                }
                                let _ = this.boss_tx.send((key, state));
                                signal_event_pump(&this.event_wake_tx);
                            }
                            BossResult::Session { session, project } if matches!(reply, BossReply::Open) => {
                                this.daemons.claim_project(project.id, key);
                                this.boss_ui.projects.insert(project.id, *project);
                                // Only `Open`'s session is the boss chat —
                                // a createPlan result names the planning
                                // session, and pointing the boss's
                                // `session_id` at it would detach the chat.
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
                            BossResult::Files { files } => {
                                if let Some((current_key, BossTab::Memory)) = this.boss_ui.page {
                                    if current_key == key && list_path.as_deref() == Some(&this.boss_ui.folder) {
                                        this.boss_ui.files = files;
                                        this.boss_ui.files_key = Some(key);
                                        this.sync_boss_page_rows();
                                    } else {
                                        this.boss_request(current_key, BossOperation::ListFiles { path: this.boss_ui.folder.clone() }, BossReply::List, cx);
                                    }
                                }
                            }
                            BossResult::File { path, content } => {
                                this.boss_ui.loaded_file = Some((key, path, content));
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
                            if !this.boss_editor_dirty(cx) {
                                this.boss_ui.editor = None;
                            }
                            this.show_toast(tr!("boss.saved"));
                            if let Some((current_key, BossTab::Memory)) = this.boss_ui.page {
                                this.boss_request(current_key, BossOperation::ListFiles { path: this.boss_ui.folder.clone() }, BossReply::List, cx);
                            } else { this.boss_request(key, BossOperation::View, BossReply::List, cx); }
                        }
                    }
                    Ok(_) => this.show_toast(tr!("boss.unexpected_response")),
                    Err(error) => this.show_toast(tr!("boss.failed", error = error.to_string())),
                }
                cx.notify();
            });
        })
        .detach();
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
        submission.prompt = if prompt.is_empty() {
            token
        } else {
            format!("{prompt} {token}")
        };
        submission.attachments.push(attachment);
        self.boss_ui.command_deliverable = None;
        self.boss_ui.deliverable_page = None;
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
        let placeholder = if self.composer_boss_command().is_some() {
            tr!("boss.command_placeholder")
        } else {
            tr!("input.do_anything")
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
    /// identity, or the boss's own for its chat. Plain tasks get `None`.
    pub(super) fn boss_session_identity(&self, session_id: Uuid) -> Option<BossIdentity> {
        if let Some(identity) = self.boss_ui.identities.get(&session_id) {
            return Some(identity.clone());
        }
        self.boss_ui
            .states
            .values()
            .find(|state| state.session_id == Some(session_id))
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
        self.session_is_boss_managed(session) && !self.session_is_boss_owned(session)
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
        // the chat transcript it covers, even when it was already selected.
        self.boss_ui.deliverable_page = None;
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
            &editor.memory,
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
        let memory = permissions.memory_folders.join("\n");
        let original = vec![
            name.clone(),
            content.clone(),
            pinned.clone(),
            memory.clone(),
        ];
        let name = input(tr!("boss.name_path"), name, false, cx);
        let content = input(tr!("boss.markdown"), content, true, cx);
        let pinned = input(tr!("boss.pinned_files"), pinned, true, cx);
        let memory = input(tr!("boss.memory_grants"), memory, true, cx);
        let focus = name.read(cx).focus();
        self.boss_ui.editor = Some(BossEditor {
            key,
            kind,
            name,
            content,
            pinned,
            memory,
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
                permissions.memory_folders = lines(editor.memory.read(cx).content());
                BossOperation::UpsertPersona {
                    persona: BossPersona {
                        id,
                        name,
                        markdown: content,
                        pinned_files: lines(editor.pinned.read(cx).content()),
                        permissions,
                        icon: editor.icon,
                    },
                }
            }
            BossEditorKind::File => BossOperation::WriteFile {
                path: name,
                content,
            },
            BossEditorKind::Folder => BossOperation::CreateFolder { path: name },
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
        let selected = state.session_id.is_some_and(|id| {
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
                    .tooltip(Tooltip::text(tr!("boss.brain")))
                    .on_activation(cx, move |this, window, cx| {
                        this.open_boss_page(key, BossTab::Memory, window, cx)
                    })
                    .child(icon("icons/brain.svg", 14.0, theme.text_secondary)),
            )
            .when_some(status_indicator, |row, indicator| row.child(indicator))
            .into_any_element()
    }

    pub(super) fn render_boss_employee_row(&self, id: Uuid, cx: &mut Context<Self>) -> AnyElement {
        let Some(identity) = self.boss_ui.identities.get(&id) else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let session = self.state.sessions.iter().find(|session| session.id == id);
        let status_indicator =
            session.and_then(|session| self.session_status_indicator(session, &theme));
        let selected = sidebar::sidebar_session_selected(
            self.state.selected_session,
            self.pending_session_activation
                .map(|pending| pending.session_id),
            id,
        );
        // Option trades the job title for the employee's model and effort,
        // the same reveal a task row's detail line performs.
        let alt_held = self.sidebar_alt_held;
        let detail = if alt_held {
            session.map(|session| self.session_sidebar_model_detail(session))
        } else {
            self.boss_ui.job_titles.get(&id).cloned()
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
                if alt_held {
                    session.map(|session| session.provider)
                } else {
                    None
                },
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
        self.boss_ui.command_deliverable = Some((key, deliverable_id));
        self.boss_ui.pending_deliverable = Some((key, deliverable_id, true));
        self.sync_composer_placeholder(cx);
        self.chat_with_boss(key, cx);
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
        self.boss_ui.deliverable_page = Some((key, deliverable_id));
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
        if self
            .boss_ui
            .loaded_file
            .as_ref()
            .is_some_and(|(key, _, _)| self.boss_ui.page == Some((*key, BossTab::Memory)))
            && let Some((key, path, content)) = self.boss_ui.loaded_file.take()
        {
            let window_handle = self.window_handle;
            let waku = cx.entity();
            cx.defer(move |cx| {
                let _ = window_handle.update(cx, move |_, window, cx| {
                    waku.update(cx, |this, cx| {
                        if this.boss_ui.page == Some((key, BossTab::Memory)) {
                            this.edit_boss_document(
                                key,
                                BossEditorKind::File,
                                path,
                                content,
                                None,
                                window,
                                cx,
                            );
                        } else {
                            this.boss_ui.loaded_file = Some((key, path, content));
                        }
                    });
                });
            });
        }
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
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .p(px(16.0))
            .border_b_1()
            .border_color(theme.border)
            .child(div().flex_1().text_size(sp(18.0)).child(name.clone()))
            .child(
                boss_button("boss-chat", tr!("boss.chat"), &theme)
                    .child(icon("icons/message-square.svg", 14.0, theme.text_secondary))
                    .child(tr!("boss.chat"))
                    .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx)),
            )
            .children(
                [
                    (BossTab::History, "boss.history"),
                    (BossTab::Memory, "boss.memory"),
                    (BossTab::Plans, "boss.plans"),
                    (BossTab::Personas, "boss.personas"),
                ]
                .into_iter()
                .map(|(target, label)| {
                    let glyph = match target {
                        BossTab::History => "icons/folder-clock.svg",
                        BossTab::Memory => "icons/brain.svg",
                        BossTab::Plans => "icons/map.svg",
                        BossTab::Personas => "icons/user-round.svg",
                    };
                    boss_button(label, tr!(label), &theme)
                        .when(tab == target, |button| button.bg(theme.overlay))
                        .child(icon(glyph, 14.0, theme.text_secondary))
                        .child(tr!(label))
                        .on_activation(cx, move |this, window, cx| {
                            this.open_boss_page(key, target, window, cx)
                        })
                }),
            );
        let mut toolbar = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(20.0))
            .py(px(10.0));
        match tab {
            BossTab::History => {
                toolbar = toolbar
                    .child(
                        div()
                            .flex_1()
                            .text_color(theme.text_secondary)
                            .child(tr!("boss.history_hint")),
                    )
                    .child(
                        boss_button("boss-rename", tr!("boss.rename"), &theme)
                            .child(tr!("boss.rename"))
                            .on_activation(cx, move |this, window, cx| {
                                this.edit_boss_document(
                                    key,
                                    BossEditorKind::Name,
                                    name.clone(),
                                    String::new(),
                                    None,
                                    window,
                                    cx,
                                )
                            }),
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
            BossTab::Memory => {
                toolbar = toolbar
                    .child(icon("icons/folder-open.svg", 15.0, theme.text_tertiary))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(theme.text_secondary)
                            .child(self.boss_ui.folder.clone()),
                    )
                    .child(
                        boss_button("boss-parent", tr!("boss.parent"), &theme)
                            .child(icon("icons/arrow-up.svg", 14.0, theme.text_secondary))
                            .child(tr!("boss.parent"))
                            .on_activation(cx, move |this, _, cx| {
                                if this.boss_editor_dirty(cx) {
                                    this.show_toast(tr!("boss.save_first"));
                                    return;
                                }
                                let parent = this
                                    .boss_ui
                                    .folder
                                    .rsplit_once('/')
                                    .map(|(parent, _)| parent.to_owned())
                                    .unwrap_or_default();
                                this.boss_ui.folder = parent.clone();
                                this.boss_request(
                                    key,
                                    BossOperation::ListFiles { path: parent },
                                    BossReply::List,
                                    cx,
                                );
                            }),
                    )
                    .children(
                        [(false, "boss.new_file"), (true, "boss.new_folder")]
                            .into_iter()
                            .map(|(folder, label)| {
                                boss_button(label, tr!(label), &theme)
                                    .child(icon(
                                        if folder {
                                            "icons/folder-new.svg"
                                        } else {
                                            "icons/plus.svg"
                                        },
                                        14.0,
                                        theme.text_secondary,
                                    ))
                                    .child(tr!(label))
                                    .on_activation(cx, move |this, window, cx| {
                                        let path = format!("{}/", this.boss_ui.folder)
                                            .trim_start_matches('/')
                                            .to_owned();
                                        this.edit_boss_document(
                                            key,
                                            if folder {
                                                BossEditorKind::Folder
                                            } else {
                                                BossEditorKind::File
                                            },
                                            path,
                                            String::new(),
                                            None,
                                            window,
                                            cx,
                                        );
                                    })
                            }),
                    );
            }
        }
        let weak = cx.entity().downgrade();
        let list = div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                list(self.boss_ui.list.clone(), move |index, _, cx| {
                    weak.upgrade()
                        .map(|entity| {
                            entity.update(cx, |this, cx| this.render_boss_item(index, cx))
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
            .child(toolbar.border_b_1().border_color(theme.separator))
            .when(
                boss_loading_label_visible(self.boss_ui.pending, self.boss_ui.pending_reply),
                |element| element.child(div().px(px(20.0)).child(tr!("boss.loading"))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .w(px(300.0))
                            .flex()
                            .flex_col()
                            .min_h_0()
                            .border_r_1()
                            .border_color(theme.separator)
                            .child(list),
                    )
                    .child(if tab == BossTab::Plans {
                        self.render_boss_plan_detail(key, cx)
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

    fn render_boss_item(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = self.boss_ui.rows.get(index).cloned() else {
            return div().into_any_element();
        };
        let Some((key, _)) = self.boss_ui.page else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        match item {
            BossItem::Employee(id) => self.render_boss_employee_row(id, cx),
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
            BossItem::File(path, directory) => {
                boss_button(format!("boss-file-{index}"), path.clone(), &theme)
                    .h(px(42.0))
                    .w_full()
                    .child(icon(
                        if directory {
                            "icons/folder.svg"
                        } else {
                            "icons/file.svg"
                        },
                        16.0,
                        theme.text_secondary,
                    ))
                    .child(
                        div()
                            .truncate()
                            .child(path.rsplit('/').next().unwrap_or(&path).to_owned()),
                    )
                    .on_activation(cx, move |this, _, cx| {
                        if this.boss_editor_dirty(cx) {
                            this.show_toast(tr!("boss.save_first"));
                            cx.notify();
                            return;
                        }
                        if directory {
                            this.boss_ui.folder = path.clone();
                            this.boss_request(
                                key,
                                BossOperation::ListFiles { path: path.clone() },
                                BossReply::List,
                                cx,
                            );
                        } else {
                            this.boss_request(
                                key,
                                BossOperation::ReadFile { path: path.clone() },
                                BossReply::Read,
                                cx,
                            );
                        }
                    })
                    .into_any_element()
            }
        }
    }

    /// The Plans tab's reading pane — the selected frozen document through
    /// the same boss-memory read the session's plan tab uses.
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
        self.ensure_plan_doc(key, session_id, &plan.plan_file, cx);
        match self
            .plan_docs
            .get(&session_id)
            .and_then(|doc| doc.content.as_ref())
        {
            Some(Ok(text)) => {
                let text = text.clone();
                self.plan_document_view(session_id, &text, cx)
                    .into_any_element()
            }
            Some(Err(error)) => empty(format!("{}\n{error}", tr!("boss.plan_unavailable"))),
            None => empty(tr!("boss.loading")),
        }
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
            BossEditorKind::Name => BossTab::History,
            _ => BossTab::Memory,
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
        if matches!(
            editor.kind,
            BossEditorKind::Persona(_) | BossEditorKind::File
        ) {
            form = form
                .child(tr!("boss.markdown"))
                .child(boss_input(editor.content.clone(), &theme));
        }
        if is_persona {
            form = form
                .child(tr!("boss.memory_grants"))
                .child(boss_input(editor.memory.clone(), &theme))
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

fn boss_loading_label_visible(pending: bool, reply: Option<BossReply>) -> bool {
    pending && reply != Some(BossReply::Read)
}

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
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

pub(super) fn job_title_icon(title: &str) -> &'static str {
    let categories: &[(&[&str], &str)] = &[
        (
            &["bug", "debug", "incident", "investigator", "forensic"],
            "icons/search.svg",
        ),
        (
            &["integrator", "integration", "merge", "release"],
            "icons/git-merge.svg",
        ),
        (&["review", "reviewer", "inspector"], "icons/eye.svg"),
        (
            &["verify", "verifier", "validation", "validator"],
            "icons/circle-check.svg",
        ),
        (
            &[
                "tester",
                "testing",
                "test",
                "qa",
                "quality assurance",
                "quality engineer",
            ],
            "icons/beaker.svg",
        ),
        (
            &[
                "engineer",
                "developer",
                "programmer",
                "software",
                "coder",
                "architect",
                "devops",
                "sre",
            ],
            "icons/terminal-square.svg",
        ),
        (
            &[
                "research",
                "scientist",
                "analyst",
                "data",
                "statistician",
                "economist",
            ],
            "icons/folder-search.svg",
        ),
        (
            &[
                "builder",
                "build",
                "construction",
                "fabricator",
                "maker",
                "mechanic",
            ],
            "icons/hammer.svg",
        ),
        (
            &[
                "artist",
                "creative",
                "illustrator",
                "designer",
                "brand",
                "fashion",
                "ux",
                "ui",
                "user experience",
                "user interface",
            ],
            "icons/pencil.svg",
        ),
        (
            &[
                "writer",
                "editor",
                "author",
                "journalist",
                "copywriter",
                "documentation",
            ],
            "icons/file-text.svg",
        ),
        (
            &[
                "manager",
                "lead",
                "director",
                "executive",
                "chief",
                "supervisor",
                "producer",
            ],
            "icons/compass.svg",
        ),
        (
            &[
                "product",
                "project",
                "program",
                "operations",
                "coordinator",
                "planner",
            ],
            "icons/list.svg",
        ),
        (
            &[
                "security",
                "safety",
                "trust",
                "compliance",
                "privacy",
                "auditor",
            ],
            "icons/lock.svg",
        ),
        (
            &[
                "teacher",
                "educator",
                "instructor",
                "tutor",
                "trainer",
                "professor",
            ],
            "icons/book-open.svg",
        ),
        (
            &[
                "doctor",
                "medical",
                "nurse",
                "health",
                "therapist",
                "caregiver",
                "clinical",
            ],
            "icons/hand.svg",
        ),
        (
            &["lawyer", "legal", "counsel", "attorney", "paralegal"],
            "icons/sigma.svg",
        ),
        (
            &[
                "sales",
                "account executive",
                "business development",
                "seller",
            ],
            "icons/target.svg",
        ),
        (
            &[
                "marketing",
                "growth",
                "communications",
                "public relations",
                "community",
                "social media",
            ],
            "icons/message-square.svg",
        ),
        (
            &[
                "finance",
                "accountant",
                "bookkeeper",
                "investment",
                "treasury",
                "controller",
            ],
            "icons/chart-column.svg",
        ),
        (
            &[
                "support",
                "customer success",
                "service",
                "help desk",
                "concierge",
            ],
            "icons/headphones.svg",
        ),
        (
            &[
                "recruit",
                "talent",
                "human resources",
                "people operations",
                "hr ",
            ],
            "icons/user-round.svg",
        ),
        (
            &[
                "network",
                "infrastructure",
                "systems administrator",
                "database",
                "cloud",
            ],
            "icons/server.svg",
        ),
        (
            &[
                "logistics",
                "supply chain",
                "warehouse",
                "shipping",
                "delivery",
            ],
            "icons/package.svg",
        ),
        (
            &[
                "environment",
                "sustainability",
                "ecologist",
                "agriculture",
                "farmer",
                "botanist",
            ],
            "icons/globe.svg",
        ),
        (
            &["chef", "cook", "culinary", "hospitality", "barista"],
            "icons/coffee.svg",
        ),
        (&["founder", "entrepreneur", "startup"], "icons/zap.svg"),
        (
            &["assistant", "administrator", "secretary", "office", "clerk"],
            "icons/inbox.svg",
        ),
        (
            &["translator", "interpreter", "linguist", "localization"],
            "icons/languages.svg",
        ),
        (
            &[
                "video",
                "film",
                "photographer",
                "camera",
                "animator",
                "media",
            ],
            "icons/monitor.svg",
        ),
        (
            &["musician", "music", "sound", "audio", "composer"],
            "icons/volume-2.svg",
        ),
    ];
    categories
        .iter()
        .find(|(keywords, _)| {
            keywords
                .iter()
                .any(|keyword| title.contains_ascii_case_insensitive(keyword))
        })
        .map(|(_, icon)| *icon)
        .unwrap_or("icons/user-round.svg")
}

#[cfg(test)]
mod job_title_icon_tests {
    use super::job_title_icon;

    #[test]
    fn common_employee_roles_have_distinct_icons_without_a_bot_fallback() {
        assert_eq!(job_title_icon("Researcher"), "icons/folder-search.svg");
        assert_eq!(
            job_title_icon("Rust developer"),
            "icons/terminal-square.svg"
        );
        assert_eq!(job_title_icon("Bug investigator"), "icons/search.svg");
        assert_eq!(job_title_icon("Integrator"), "icons/git-merge.svg");
        assert_eq!(job_title_icon("Verifier"), "icons/circle-check.svg");
        assert_eq!(job_title_icon("Writer"), "icons/file-text.svg");
        assert_eq!(job_title_icon("Designer"), "icons/pencil.svg");
        assert_eq!(job_title_icon("Reviewer"), "icons/eye.svg");
        assert_eq!(job_title_icon("Tester"), "icons/beaker.svg");
        assert_eq!(job_title_icon("Uncategorized role"), "icons/user-round.svg");
    }
}

#[cfg(test)]
mod loading_indicator_tests {
    use super::{BossReply, boss_loading_label_visible};

    #[test]
    fn reading_a_memory_file_keeps_the_loading_label_hidden() {
        assert!(!boss_loading_label_visible(true, Some(BossReply::Read)));
        assert!(boss_loading_label_visible(true, Some(BossReply::List)));
        assert!(!boss_loading_label_visible(false, Some(BossReply::List)));
    }
}

trait ContainsAsciiCaseInsensitive {
    fn contains_ascii_case_insensitive(&self, needle: &str) -> bool;
}

impl ContainsAsciiCaseInsensitive for str {
    fn contains_ascii_case_insensitive(&self, needle: &str) -> bool {
        self.as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
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

}
