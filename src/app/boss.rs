//! Desktop control plane for daemon-owned Boss roles.
use super::*;
use crate::ui::ActivationExt;
use waku_client::DaemonKey;
use waku_client::boss::{
    BossFile, BossIdentity, BossOperation, BossPersona, BossResult, BossState, PersonaPermissions,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BossTab {
    History,
    Memory,
    Personas,
}

pub(super) struct BossUi {
    pub states: HashMap<DaemonKey, BossState>,
    pub projects: HashMap<Uuid, Project>,
    pub hosts: Vec<DaemonKey>,
    pub managed: HashSet<Uuid>,
    pub identities: HashMap<Uuid, BossIdentity>,
    job_titles: HashMap<Uuid, String>,
    pub active: HashMap<DaemonKey, Vec<Uuid>>,
    pub working: HashSet<Uuid>,
    pub expired: HashSet<Uuid>,
    pub recent: HashMap<DaemonKey, Vec<Uuid>>,
    pub sidebar_idle_visible: HashMap<DaemonKey, usize>,
    pub revision: u64,
    pub page: Option<(DaemonKey, BossTab)>,
    files: Vec<BossFile>,
    files_key: Option<DaemonKey>,
    folder: String,
    editor: Option<BossEditor>,
    generation: u64,
    pending: bool,
    list: ListState,
    scrollbar: Rc<ScrollbarState>,
    rows: Vec<BossItem>,
    avatar_queue: RefCell<VecDeque<String>>,
    avatar_requested: RefCell<HashSet<String>>,
    avatars: HashMap<String, Arc<gpui::RenderImage>>,
    avatar_active: usize,
    loaded_file: Option<(DaemonKey, String, String)>,
    focus: Option<FocusHandle>,
}

impl Default for BossUi {
    fn default() -> Self {
        Self {
            states: HashMap::new(),
            projects: HashMap::new(),
            hosts: Vec::new(),
            managed: HashSet::new(),
            identities: HashMap::new(),
            job_titles: HashMap::new(),
            active: HashMap::new(),
            working: HashSet::new(),
            expired: HashSet::new(),
            recent: HashMap::new(),
            sidebar_idle_visible: HashMap::new(),
            revision: 0,
            page: None,
            files: Vec::new(),
            files_key: None,
            folder: "memory".into(),
            editor: None,
            generation: 0,
            pending: false,
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum BossItem {
    Employee(Uuid),
    Persona(Uuid),
    File(String, bool),
}

struct BossEditor {
    key: DaemonKey,
    kind: BossEditorKind,
    name: Entity<TextInput>,
    content: Entity<TextInput>,
    knowledge: Entity<TextInput>,
    memory: Entity<TextInput>,
    permissions: PersonaPermissions,
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
#[derive(Clone, Copy)]
enum BossReply {
    Open,
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

    fn boss_request(
        &mut self,
        key: DaemonKey,
        operation: BossOperation,
        reply: BossReply,
        cx: &mut Context<Self>,
    ) {
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
                            &editor.knowledge,
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
                                if let Some(state) = this.boss_ui.states.get_mut(&key) { state.session_id = Some(session.id); }
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
        let id = self.state.selected_session?;
        self.boss_ui
            .states
            .iter()
            .find_map(|(key, state)| (state.session_id == Some(id)).then_some(*key))
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
            .pt(px(HEADER_HEIGHT * 2.0))
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

    fn chat_with_boss(&mut self, key: DaemonKey, cx: &mut Context<Self>) {
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
            &editor.knowledge,
            &editor.memory,
        ]
        .iter()
        .map(|input| input.read(cx).content().to_owned())
        .collect::<Vec<_>>();
        values != editor.original
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
        let knowledge = persona
            .as_ref()
            .map(|entry| entry.knowledge_files.join("\n"))
            .unwrap_or_default();
        let permissions = persona.map(|entry| entry.permissions).unwrap_or_default();
        let memory = permissions.memory_folders.join("\n");
        let original = vec![
            name.clone(),
            content.clone(),
            knowledge.clone(),
            memory.clone(),
        ];
        let name = input(tr!("boss.name_path"), name, false, cx);
        let content = input(tr!("boss.markdown"), content, true, cx);
        let knowledge = input(tr!("boss.knowledge"), knowledge, true, cx);
        let memory = input(tr!("boss.memory_grants"), memory, true, cx);
        let focus = name.read(cx).focus();
        self.boss_ui.editor = Some(BossEditor {
            key,
            kind,
            name,
            content,
            knowledge,
            memory,
            original,
            original_permissions: permissions.clone(),
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
                        knowledge_files: lines(editor.knowledge.read(cx).content()),
                        permissions,
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

    fn boss_avatar(&self, identity: &BossIdentity, size: f32, cx: &App) -> AnyElement {
        if let Some(image) = self.boss_ui.avatars.get(&identity.avatar_seed) {
            return gpui::img(image.clone())
                .size(px(size))
                .rounded(px(6.0))
                .into_any_element();
        }
        if self
            .boss_ui
            .avatar_requested
            .borrow_mut()
            .insert(identity.avatar_seed.clone())
        {
            self.boss_ui
                .avatar_queue
                .borrow_mut()
                .push_back(identity.avatar_seed.clone());
            signal_event_pump(&self.event_wake_tx);
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
            let Some(seed) = self.boss_ui.avatar_queue.borrow_mut().pop_front() else {
                break;
            };
            self.boss_ui.avatar_active += 1;
            let http = cx.http_client();
            let renderer = cx.svg_renderer();
            let avatar_seed = seed.clone();
            cx.spawn(async move |this, cx| {
                let image = cx.background_executor().spawn(async move {
                    use futures::io::AsyncReadExt;
                    let exchange = async {
                        let url = format!("https://api.dicebear.com/10.x/moods/svg?backgroundColor=&tags=animation&seed={avatar_seed}");
                        let request = gpui::http_client::Request::get(url).body(gpui::http_client::AsyncBody::empty())?;
                        let mut response = http.send(request).await?;
                        anyhow::ensure!(response.status().is_success(), "avatar unavailable");
                        let mut bytes = Vec::new();
                        response.body_mut().take(256 * 1024).read_to_end(&mut bytes).await?;
                        Ok::<_, anyhow::Error>(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| renderer.render_single_frame(&bytes, 1.0)))
                            .map_err(|_| anyhow::anyhow!("avatar rendering failed"))??)
                    };
                    match futures::future::select(Box::pin(exchange), Box::pin(smol::Timer::after(std::time::Duration::from_secs(10)))).await {
                        futures::future::Either::Left((result, _)) => result.ok(), _ => None,
                    }
                }).await;
                let _ = this.update(cx, |this, cx| {
                    this.boss_ui.avatar_active -= 1;
                    if let Some(image) = image {
                        if this.boss_ui.avatars.len() >= 256 {
                            if let Some(old) = this.boss_ui.avatars.keys().next().cloned() {
                                if let Some(image) = this.boss_ui.avatars.remove(&old) { cx.drop_image(image, None); }
                                this.boss_ui.avatar_requested.borrow_mut().remove(&old);
                            }
                        }
                        this.boss_ui.avatars.insert(seed, image);
                    }
                    signal_event_pump(&this.event_wake_tx); cx.notify();
                });
            }).detach();
        }
    }

    pub(super) fn boss_employee_finished(&self) -> bool {
        self.state
            .selected_session
            .is_some_and(|id| self.boss_ui.expired.contains(&id))
    }

    pub(super) fn render_boss_finished_footer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.session_surface_active() || !self.boss_employee_finished() {
            return None;
        }
        let id = self.state.selected_session?;
        let key = self.daemons.session_owner(id);
        let theme = Theme::current(cx);
        Some(
            div()
                .p(px(16.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .border_t_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex_1()
                        .text_color(theme.text_secondary)
                        .child(tr!("boss.expired_hint")),
                )
                .child(
                    boss_button("employee-boss-chat", tr!("boss.chat"), &theme)
                        .child(tr!("boss.chat"))
                        .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx)),
                )
                .into_any_element(),
        )
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
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx))
            .child(self.boss_avatar(&state.identity, 24.0, cx))
            .child(boss_sidebar_label(
                state.identity.name.clone(),
                tr!("boss.group"),
                &theme,
            ))
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
            .into_any_element()
    }

    pub(super) fn render_boss_employee_row(&self, id: Uuid, cx: &mut Context<Self>) -> AnyElement {
        let Some(identity) = self.boss_ui.identities.get(&id) else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let active = self.boss_ui.working.contains(&id);
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
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .on_activation(cx, move |this, _, cx| {
                this.request_session_activation(id, SessionActivationTransition::Visit, cx)
            })
            .child(self.boss_avatar(identity, 24.0, cx))
            .child(boss_sidebar_label(
                identity.name.clone(),
                self.boss_ui
                    .job_titles
                    .get(&id)
                    .cloned()
                    .unwrap_or_default(),
                &theme,
            ))
            .child(
                div()
                    .text_size(sp(10.0))
                    .text_color(theme.text_tertiary)
                    .child(if active {
                        tr!("boss.working")
                    } else {
                        tr!("boss.finished")
                    }),
            )
            .into_any_element()
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
                    .child(tr!("boss.chat"))
                    .on_activation(cx, move |this, _, cx| this.chat_with_boss(key, cx)),
            )
            .children(
                [
                    (BossTab::History, "boss.history"),
                    (BossTab::Memory, "boss.memory"),
                    (BossTab::Personas, "boss.personas"),
                ]
                .into_iter()
                .map(|(target, label)| {
                    boss_button(label, tr!(label), &theme)
                        .when(tab == target, |button| button.bg(theme.overlay))
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
                    .child(div().flex_1().truncate().child(self.boss_ui.folder.clone()))
                    .child(
                        boss_button("boss-parent", tr!("boss.parent"), &theme)
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
            .child(toolbar)
            .when(self.boss_ui.pending, |element| {
                element.child(div().px(px(20.0)).child(tr!("boss.loading")))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(div().w(px(300.0)).flex().flex_col().min_h_0().child(list))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .id("boss-editor-scroll")
                            .overflow_y_scroll()
                            .p(px(20.0))
                            .child(editor),
                    ),
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
            .child(editor.name.clone());
        if matches!(
            editor.kind,
            BossEditorKind::Persona(_) | BossEditorKind::File
        ) {
            form = form
                .child(tr!("boss.markdown"))
                .child(editor.content.clone());
        }
        if is_persona {
            form = form
                .child(tr!("boss.knowledge"))
                .child(editor.knowledge.clone())
                .child(tr!("boss.memory_grants"))
                .child(editor.memory.clone())
                .child(tr!("boss.permissions_hint"));
            for (computer, label, enabled) in [
                (false, "boss.delegate", editor.permissions.summon_employees),
                (true, "boss.computer", editor.permissions.computer_use),
            ] {
                form = form.child(
                    boss_button(label, tr!(label), &theme)
                        .child(format!(
                            "{} {}",
                            if enabled { "☑" } else { "☐" },
                            tr!(label)
                        ))
                        .on_activation(cx, move |this, _, cx| {
                            if let Some(editor) = &mut this.boss_ui.editor {
                                if computer {
                                    editor.permissions.computer_use =
                                        !editor.permissions.computer_use;
                                } else {
                                    editor.permissions.summon_employees =
                                        !editor.permissions.summon_employees;
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
                            .child(format!("{} {}", if enabled { "☑" } else { "☐" }, id))
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
                        .child(tr!("boss.save"))
                        .on_activation(cx, |this, _, cx| this.save_boss_document(cx)),
                )
                .child(
                    boss_button("boss-discard", tr!("boss.discard"), &theme)
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

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[track_caller]
fn boss_sidebar_label(name: String, job_title: String, theme: &Theme) -> Div {
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
                .text_size(sp(13.0))
                .line_height(sp(15.0))
                .text_color(theme.text_tertiary)
                .truncate()
                .child(job_title),
        )
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
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(sp(12.0))
        .text_color(theme.text)
        .cursor_pointer()
        .hover(|style| style.bg(theme.overlay))
        .focus_visible(|style| style.bg(theme.focus_highlight()))
}
