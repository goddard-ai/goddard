use super::*;

impl WakuBackend {
    /// Whether `session_id` names a task the daemon knows.
    pub(crate) fn known_session(&self, session_id: Uuid) -> bool {
        self.task_state
            .lock()
            .sessions
            .iter()
            .any(|session| session.id == session_id)
    }

    /// Still open for prompts — present and unarchived. `BossService` reads
    /// it to decide whether a planning supervisor can take a report.
    pub(crate) fn session_active(&self, session_id: Uuid) -> bool {
        self.task_state
            .lock()
            .sessions
            .iter()
            .any(|session| session.id == session_id && session.archived_at.is_none())
    }

    /// The whole agent surface sits behind this daemon-level setting. While
    /// it is off the commands are rejected for every caller — scoped
    /// credentials included — and nothing is minted or injected.
    pub(super) fn require_agent_tools(&self) -> anyhow::Result<()> {
        if !self.settings.get().agent_tools_enabled {
            bail!("agent session commands are disabled on this daemon");
        }
        Ok(())
    }

    /// A scoped `rename` either applies outright — the task holds a stored
    /// grant — or parks a permission request on the session until the user
    /// answers. The request survives the turn that raised it: it is parked
    /// on the session, not the turn, so a folded turn cannot hide it.
    pub(super) fn agent_rename_self(
        &self,
        agent: Option<Uuid>,
        title: &str,
        events: &EventSink,
    ) -> anyhow::Result<ResponsePayload> {
        let caller =
            agent.ok_or_else(|| anyhow!("agent rename requires a scoped task credential"))?;
        if self.boss.is_managed(caller) {
            bail!("Boss identity names are controlled through Boss operations");
        }
        if title.trim().is_empty() {
            bail!("a task title cannot be empty");
        }
        {
            let mut state = self.task_state.lock();
            let session = state
                .session_mut(caller)
                .ok_or_else(|| anyhow!("task {caller} is unknown to the daemon"))?;
            if session.agent_rename_allowed {
                if session.set_title(title) {
                    self.task_store.save(&mut state)?;
                }
                return Ok(ResponsePayload::Ack);
            }
        }
        let runtime_id = self
            .sessions
            .lock()
            .get(&caller)
            .map(|entry| entry.runtime_id)
            .ok_or_else(|| anyhow!("task {caller} has no running runtime to show the request"))?;
        let current = self
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == caller)
            .map(|session| session.title.clone())
            .unwrap_or_default();
        let request_id = format!(
            "{}{}",
            waku_protocol::AGENT_RENAME_REQUEST_PREFIX,
            Uuid::new_v4()
        );
        let (title_text, title_i18n) = localized!("session.agent_rename_request", title = title);
        let (detail_text, detail_i18n) =
            localized!("session.agent_rename_request_from", current = current);
        let wire = event_to_wire(DriverEvent::Permission {
            request_id: request_id.clone(),
            title: title_text,
            title_i18n: Some(title_i18n),
            detail: detail_text,
            detail_i18n: Some(detail_i18n),
            options: vec![
                PermissionOption::keyed("once", localized!("session.agent_rename_once"), true),
                PermissionOption::keyed("always", localized!("session.agent_rename_always"), true),
                PermissionOption::keyed("deny", localized!("common.deny"), false),
            ],
        })?;
        let (settled, settle_rx) = crossbeam_channel::bounded(1);
        // The card holds one request — a parallel rename would hide the
        // first behind it and park forever, so the second is refused.
        if !self
            .agent
            .try_park_permission(caller, request_id.clone(), settled)
        {
            bail!("task {caller} already has a rename request waiting on the user");
        }
        let events = events.for_session(caller, runtime_id);
        if let Err(error) = events.send(wire) {
            self.agent.remove_permission(caller, &request_id);
            return Err(error);
        }
        // Parked like a provider request: the user's response resolves it —
        // from any client — and an exited process or torn-down session
        // resolves it unanswered. A finished turn does not.
        let option = settle_rx.recv().unwrap_or_default();
        self.agent.remove_permission(caller, &request_id);
        // Clients that never saw the answer still drop the card.
        let _ = events.send(event_to_wire(DriverEvent::RequestSettled { request_id })?);
        match option.as_deref() {
            Some("once") | Some("always") => {
                let mut state = self.task_state.lock();
                let session = state
                    .session_mut(caller)
                    .ok_or_else(|| anyhow!("task {caller} is unknown to the daemon"))?;
                // Set the title first — `granted || set_title` would skip the
                // rename entirely on an "always" answer.
                let renamed = session.set_title(title);
                let granted = option.as_deref() == Some("always") && !session.agent_rename_allowed;
                if granted {
                    session.agent_rename_allowed = true;
                }
                if renamed || granted {
                    self.task_store.save(&mut state)?;
                }
                Ok(ResponsePayload::Ack)
            }
            Some(_) => bail!("the rename to {title:?} was declined"),
            None => bail!("the rename request went unanswered"),
        }
    }

    /// A scoped `archive` is a proposal, never an action: the daemon parks a
    /// permission card on the calling session and only archives what the
    /// user approves. Targets must be started, unarchived tasks in the
    /// caller's own project — the same reach the agent's `search` has —
    /// because the card can only meaningfully list a set the caller could
    /// have found. The boss principal is the exception `search` already
    /// grants: its own project holds only its session, so it may name a
    /// task in any project the daemon knows and the card names each
    /// task's project beside its title. Side chats are refused by name;
    /// archiving their parent already takes them.
    pub(super) fn agent_propose_archive(
        &self,
        agent: Option<Uuid>,
        task_ids: Vec<Uuid>,
        reason: Option<String>,
        events: &EventSink,
    ) -> anyhow::Result<ResponsePayload> {
        let caller =
            agent.ok_or_else(|| anyhow!("archive proposals require a scoped task credential"))?;
        self.require_agent_tools()?;
        if task_ids.is_empty() {
            bail!("an archive proposal names at least one task");
        }
        if task_ids.len() > AGENT_ARCHIVE_PROPOSAL_MAX_TASKS {
            bail!("an archive proposal names at most {AGENT_ARCHIVE_PROPOSAL_MAX_TASKS} tasks");
        }
        let reason = reason
            .map(|reason| reason.trim().to_owned())
            .filter(|reason| !reason.is_empty());
        if reason
            .as_ref()
            .is_some_and(|reason| reason.chars().count() > AGENT_ARCHIVE_PROPOSAL_MAX_REASON_CHARS)
        {
            bail!(
                "an archive reason is at most {AGENT_ARCHIVE_PROPOSAL_MAX_REASON_CHARS} characters"
            );
        }
        let mut unique = Vec::with_capacity(task_ids.len());
        for id in task_ids {
            if !unique.contains(&id) {
                unique.push(id);
            }
        }
        // Like `agent_search_sessions`, the boss principal's project is a
        // placeholder holding only its session, so its proposals reach a
        // task in any project the daemon knows. Every other caller stays
        // confined to the project its own session belongs to.
        let boss_principal = self.boss.is_boss_principal(caller);
        let (target_ids, titles, projects) = {
            let state = self.task_state.lock();
            let project_id = state
                .sessions
                .iter()
                .find(|session| session.id == caller)
                .map(|session| session.project_id)
                .ok_or_else(|| anyhow!("task {caller} is unknown to the daemon"))?;
            let mut ids = Vec::with_capacity(unique.len());
            let mut titles = Vec::with_capacity(unique.len());
            let mut projects = Vec::with_capacity(unique.len());
            for id in &unique {
                let session = state
                    .sessions
                    .iter()
                    .find(|session| session.id == *id)
                    .ok_or_else(|| anyhow!("task {id} is unknown to the daemon"))?;
                if !boss_principal && session.project_id != project_id {
                    bail!("task {id} is outside this task's project");
                }
                if session.is_side_chat() {
                    bail!("task {id} is a side chat — propose its parent task instead");
                }
                if session.archived_at.is_some() {
                    bail!("task {id} is already archived");
                }
                if !session.has_started() {
                    bail!("task {id} has not started");
                }
                ids.push(*id);
                titles.push(session.display_title().to_owned());
                // A boss card can span projects, so each named task also
                // names the project the user is asked to archive it in.
                projects.push(if boss_principal {
                    state
                        .projects
                        .iter()
                        .find(|project| project.id == session.project_id)
                        .map(|project| project.name.clone())
                } else {
                    None
                });
            }
            (ids, titles, projects)
        };
        let runtime_id = self
            .sessions
            .lock()
            .get(&caller)
            .map(|entry| entry.runtime_id)
            .ok_or_else(|| anyhow!("task {caller} has no running runtime to show the request"))?;
        let request_id = format!(
            "{}{}",
            waku_protocol::AGENT_ARCHIVE_REQUEST_PREFIX,
            Uuid::new_v4()
        );
        let count = titles.len();
        let (title_text, title_i18n) = if count == 1 {
            localized!("archive.confirm_title_named", name = &titles[0])
        } else {
            localized!("session.agent_archive_request_many", count = count)
        };
        let mut tasks_text = titles
            .iter()
            .zip(&projects)
            .take(AGENT_ARCHIVE_CARD_TITLES)
            .map(|(title, project)| match project {
                Some(project) => format!("“{title}” ({project})"),
                None => format!("“{title}”"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        if titles.len() > AGENT_ARCHIVE_CARD_TITLES {
            tasks_text.push_str(", …");
        }
        let (detail_text, detail_i18n) = match &reason {
            Some(reason) => localized!(
                "session.agent_archive_request_detail_reason",
                tasks = &tasks_text,
                reason = reason
            ),
            None => localized!("session.agent_archive_request_detail", tasks = &tasks_text),
        };
        let wire = event_to_wire(DriverEvent::Permission {
            request_id: request_id.clone(),
            title: title_text,
            title_i18n: Some(title_i18n),
            detail: detail_text,
            detail_i18n: Some(detail_i18n),
            options: vec![
                PermissionOption::keyed("archive", localized!("session.archive"), true),
                PermissionOption::keyed("deny", localized!("common.deny"), false),
            ],
        })?;
        let (settled, settle_rx) = crossbeam_channel::bounded(1);
        // The pinned card holds one daemon request per session — a parallel
        // proposal would hide the first behind it and park forever.
        if !self
            .agent
            .try_park_permission(caller, request_id.clone(), settled)
        {
            bail!("task {caller} already has a request waiting on the user");
        }
        let events = events.for_session(caller, runtime_id);
        if let Err(error) = events.send(wire) {
            self.agent.remove_permission(caller, &request_id);
            return Err(error);
        }
        // Parked like a rename request: an answer from any client resolves
        // it; a finished turn does not. Approval archives daemon-side, so
        // every client — the answerer's included — resyncs through the
        // task-state bump this request's response triggers.
        let option = settle_rx.recv().unwrap_or_default();
        self.agent.remove_permission(caller, &request_id);
        let _ = events.send(event_to_wire(DriverEvent::RequestSettled { request_id })?);
        match option.as_deref() {
            Some("archive") => {
                // Revalidate under the lock inside `archive_sessions`: a task
                // the user archived while this card waited simply skips.
                self.archive_sessions(&target_ids)?;
                Ok(ResponsePayload::Ack)
            }
            Some(_) => bail!("the archive proposal was declined"),
            None => bail!("the archive request went unanswered"),
        }
    }

    /// `createPlan`: open a boss-attached planning session on an idea. Only
    /// a boss principal or a human may start one — employees never spawn
    /// sibling plans, and there is no user-initiated creation path beyond
    /// this operation. The session joins the Boss project, is stamped
    /// `boss_managed` with its `planning` marker, and its transcript seeds
    /// with the user request plus a localized opener asking the boss to
    /// explain its understanding.
    pub(super) fn create_plan(
        &self,
        caller: Option<Uuid>,
        title: String,
        plan_file: String,
        prompt: String,
        provider: Option<ProviderKind>,
        model: Option<String>,
        reasoning_effort: Option<String>,
        events: &EventSink,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        use waku_protocol::boss::BossResult;
        if caller.is_some_and(|id| !self.boss.is_boss_principal(id)) {
            bail!("only the boss can start a planning session");
        }
        self.boss.with_operation_lock(|| {
            let title = title.trim().to_owned();
            if title.is_empty() {
                bail!("a planning session needs a title");
            }
            if prompt.trim().is_empty() {
                bail!("a planning session needs the user prompt that prompted it");
            }
            let plan_file = crate::boss::normalize_plan_file(&plan_file)?;
            let boss = self.boss.document();
            // Two sessions cannot share one document — a name the registry
            // already holds would fork writes across both.
            if self.boss.plan_for_file(&plan_file).is_some() {
                bail!("a plan named {plan_file} already exists");
            }
            // The planning session lives in the Boss project with the boss
            // chat. `create_agent_task_inner` resolves projects by path, so the
            // Boss project must already name this path — register it the same
            // way `Open` does when it has not been.
            let workspace = dunce::canonicalize(&self.boss.owned_workspace()?)?;
            let project = {
                let mut state = self.task_state.lock();
                let found = state
                    .projects
                    .iter_mut()
                    .find(|entry| {
                        entry.id == boss.identity.id
                            || dunce::canonicalize(&entry.path).is_ok_and(|path| path == workspace)
                    })
                    .cloned();
                match found {
                    Some(project) => project,
                    None => {
                        let mut project = Project::from_path(workspace.clone());
                        project.id = boss.identity.id;
                        project.name = "Boss".into();
                        state.projects.push(project.clone());
                        project
                    }
                }
            };
            let sender = caller.or(boss.session_id);
            let plan = waku_protocol::boss::BossPlan {
                id: Uuid::new_v4(),
                session_id: Uuid::new_v4(),
                plan_file: plan_file.clone(),
                idea: title.clone(),
                finalized_at: None,
                items: Vec::new(),
                outcome: None,
                history: Vec::new(),
            };
            let (opener, _) = localized!("boss.plan_seed_opener", path = plan_file.clone());
            let storage = self.boss.plan_file_context(&plan_file)?;
            let seed = format!("{}\n\n{}\n\n{}", prompt.trim(), opener, storage);
            // Planning is design/drafting work: it defaults to Codex's sol
            // model at medium effort rather than inheriting the boss
            // chat's own pick. Each field the caller supplies still wins —
            // a `provider` override without a `model` keeps that
            // provider's own default model.
            let provider = provider.or(Some(PLANNING_PROVIDER));
            let model = model.or_else(|| {
                (provider == Some(PLANNING_PROVIDER)).then(|| PLANNING_MODEL.to_owned())
            });
            let selection = AgentCreateSelection {
                provider,
                model,
                title: Some(title),
                reasoning_effort: reasoning_effort.or_else(|| Some(PLANNING_EFFORT.to_owned())),
                service_tier: None,
                context_window: None,
            };
            let session_id = self.create_agent_task_inner(
                sender,
                selection,
                workspace,
                AgentWorkspace::Local,
                None,
                AgentTaskPrompt::Fixed(seed),
                events,
                None,
                Some(plan),
            )?;
            let session = self
                .task_state
                .lock()
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .cloned()
                .ok_or_else(|| anyhow!("planning session {session_id} is missing"))?;
            Ok(BossResult::Session {
                session: Box::new(session),
                project: Box::new(project),
            })
        })
    }

    /// `finalizePlan`: freeze a plan document once the user approves. A
    /// planning session finalizes its own plan; the boss chat or a human
    /// names the file. Scoped callers park a daemon-owned approval card on
    /// the boss chat — the same contract `agentProposeArchive` uses — and
    /// only the `finalize` answer stamps `finalized_at`, freezes writes,
    /// and starts the post-approval grace period before archival. A human
    /// caller is the approver, so a master-token call finalizes directly.
    /// Freezing also queues the implementation handoff on the boss chat —
    /// a durable hidden prompt naming the frozen doc — so the boss
    /// coordinates the work from one surface.
    pub(super) fn finalize_plan(
        &self,
        caller: Option<Uuid>,
        plan_file: Option<String>,
        items: Option<Vec<String>>,
        events: &EventSink,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        use waku_protocol::boss::BossResult;
        // Plan finalization is the human's call alone: agents can neither
        // approve their own work nor request the approval. The boundary
        // sits here at the daemon so every scoped path — boss op, eval
        // dispatch, agent CLI — lands on it and no request card is ever
        // created. The user-facing control calls with no caller.
        if caller.is_some() {
            bail!("plan finalization is user-only — use the approve control in the app");
        }
        // Resolve the plan: an explicit file names the record, while a
        // planning session's bare `finalizePlan` means its own.
        let plan = match plan_file {
            Some(plan_file) => {
                let plan_file = crate::boss::normalize_plan_file(&plan_file)?;
                if let Some(caller) = caller
                    && self.boss.is_planning(caller)
                    && self
                        .boss
                        .plan(caller)
                        .is_some_and(|own| own.plan_file != plan_file)
                {
                    bail!("a planning session finalizes only its own plan");
                }
                self.boss
                    .plan_for_file(&plan_file)
                    .ok_or_else(|| anyhow!("no plan named {plan_file}"))?
            }
            None => {
                let caller = caller.ok_or_else(|| {
                    anyhow!("finalizePlan needs a plan file when it does not come from a planning session")
                })?;
                self.boss
                    .plan(caller)
                    .ok_or_else(|| anyhow!("this session owns no plan to finalize"))?
            }
        };
        if plan.finalized_at.is_some() {
            bail!("plan {} is already finalized", plan.plan_file);
        }
        {
            let state = self.task_state.lock();
            if !state
                .sessions
                .iter()
                .any(|session| session.id == plan.session_id && session.archived_at.is_none())
            {
                bail!("planning session for {} is gone", plan.plan_file);
            }
        }
        let finalized =
            self.boss
                .finalize_plan(&plan.plan_file, items, crate::model::unix_time())?;
        {
            let mut state = self.task_state.lock();
            if let Some(session) = state
                .sessions
                .iter_mut()
                .find(|session| session.id == plan.session_id)
            {
                if let Some(planning) = session.planning.as_mut() {
                    planning.finalized_at = finalized.finalized_at;
                }
                session.updated_at = crate::model::unix_time();
                state.mark_session_dirty(plan.session_id);
                self.task_store.save(&mut state)?;
            }
        }
        // The next prompt re-injects the persona block so the agent learns
        // the plan froze.
        self.boss.reset_context(plan.session_id);
        // Approval hands implementation to the boss. Deliver into an open
        // turn immediately; otherwise the hidden prompt is parked durably.
        if let Some(boss_session) = self.boss.document().session_id {
            let handoff = format!(
                "Planning session \"{}\" ({}) finalized its design at {} — the document is approved and frozen. Immediately confirm to the human that you received the finalized plan, then coordinate its implementation from here: summon employees for the work and keep the human posted. The planning session stays open during its grace period to answer questions about the design.",
                finalized.idea, plan.session_id, finalized.plan_file,
            );
            let trigger = crate::model::ReportTrigger::plan_finalized(&finalized);
            if let Err(error) = self.deliver_employee_report(
                boss_session,
                handoff,
                plan.session_id,
                Some(trigger),
                events,
            ) {
                eprintln!("could not hand the finalized plan to the boss chat: {error:#}");
            }
        }
        Ok(BossResult::PlanFinalized {
            session_id: plan.session_id,
            plan_file: finalized.plan_file,
            finalized_at: finalized.finalized_at.unwrap_or_default(),
        })
    }

    /// The settings surface is gated separately from task creation and only
    /// for scoped credentials — a client holding the master token already has
    /// full `updateSettings` access, so the flag must not gate it.
    pub(super) fn require_agent_settings(&self, agent: Option<Uuid>) -> anyhow::Result<()> {
        if agent.is_some_and(|id| self.boss.is_managed(id)) {
            bail!("Boss roles cannot change daemon settings");
        }
        if agent.is_some() && !self.settings.get().agent_settings_enabled {
            bail!("agent settings commands are disabled on this daemon");
        }
        Ok(())
    }

    /// Apply `command` to the daemon-owned list. An upsert keys on the id
    /// first and the exact name second, so an agent can assert "this command
    /// exists" without tracking list state; a nil id always mints a new
    /// command. Agent writes stamp `created_by_task` so clients can show
    /// where the entry came from.
    pub(super) fn upsert_custom_command(
        &self,
        agent: Option<Uuid>,
        mut command: CustomCommand,
    ) -> anyhow::Result<Vec<CustomCommand>> {
        if command.script.trim().is_empty() {
            bail!("custom commands require a script");
        }
        // Agent writes are stamped with their task; a client keeps whatever
        // attribution the command already carries.
        if agent.is_some() {
            command.created_by_task = agent;
        }
        if command.id.is_nil() {
            command.id = Uuid::new_v4();
        }
        let mut settings = self.settings.get();
        let existing = settings.custom_commands.iter().position(|existing| {
            existing.id == command.id
                || command
                    .name
                    .as_deref()
                    .is_some_and(|name| existing.name.as_deref() == Some(name))
        });
        match existing {
            Some(index) => settings.custom_commands[index] = command,
            None => settings.custom_commands.push(command),
        }
        self.settings.replace(settings)?;
        Ok(self.settings.get().custom_commands)
    }

    pub(super) fn remove_custom_command(
        &self,
        id: Option<Uuid>,
        name: Option<String>,
    ) -> anyhow::Result<Vec<CustomCommand>> {
        let name = name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        if id.is_none() && name.is_none() {
            bail!("custom command removal needs an id or a name");
        }
        let mut settings = self.settings.get();
        let before = settings.custom_commands.len();
        settings.custom_commands.retain(|command| {
            !(id.is_some_and(|id| command.id == id)
                || name
                    .as_deref()
                    .is_some_and(|name| command.name.as_deref() == Some(name)))
        });
        if settings.custom_commands.len() == before {
            bail!("no custom command matches");
        }
        self.settings.replace(settings)?;
        Ok(self.settings.get().custom_commands)
    }
}
