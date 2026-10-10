use super::*;

impl WakuBackend {
    /// Persisted quarantine flag — set on received-file sessions until the
    /// user trusts the transfer. Checked against `task_state`, not the
    /// running-driver map, so it holds for sessions that aren't running.
    pub(crate) fn session_quarantined(&self, session_id: Uuid) -> bool {
        let mut state = self.task_state.lock();
        let Some(index) = state
            .sessions
            .iter()
            .position(|session| session.id == session_id)
        else {
            return false;
        };
        // Quarantine lives in the session detail, not the list row — a
        // skeleton answers false until hydrated, so load before judging.
        if !state.sessions[index].detail_loaded {
            let _ = self.task_store.hydrate(&mut state.sessions[index]);
        }
        state.sessions[index].quarantined
    }

    /// The daemon's snapshot of the user's work — projects, live tasks,
    /// employees, and automations — as the context router both evaluates
    /// against and attaches.
    pub(super) fn boss_work_context(&self) -> crate::boss_context::WorkContext {
        crate::boss_context::work_context(
            &self.task_state.lock(),
            &self.boss.document(),
            &self.automations.document(),
        )
    }

    /// The boss-facing wrap for an outbound prompt: the persona injection
    /// first, then the always-on work header — replaced by the deferred
    /// full digest when the context router asked for one a steer-less or
    /// already-settled turn could not take.
    pub(super) fn boss_outbound_prompt(&self, session_id: Uuid, prompt: String) -> String {
        // A rotated chat's first human prompt carries the same durable
        // handoff as the daemon's queued-prompt path.
        let handoff = if self.boss.is_boss(session_id) {
            let mut state = self.task_state.lock();
            let handoff = state
                .session_mut(session_id)
                .and_then(|session| session.pending_provider_context.take());
            if handoff.is_some() {
                if let Err(error) = self.task_store.save(&mut state) {
                    eprintln!("could not persist Boss context delivery: {error:#}");
                }
            }
            handoff
        } else {
            None
        };
        let prompt = match handoff {
            Some(context) if !prompt.contains(&context) => format!("{context}\n\n{prompt}"),
            _ => prompt,
        };
        wrap_boss_outbound_prompt(
            &self.task_state,
            &self.automations.document(),
            &self.boss,
            session_id,
            prompt,
        )
    }

    /// Evaluate whether a user prompt should carry the full work digest.
    /// Failed or unconfigured evaluations keep only the always-on header.
    pub(super) fn route_boss_prompt(&self, session_id: Uuid, prompt: &str) {
        let recent_prompts = self.boss.router_snapshot(session_id);
        self.boss.router_note_prompt(session_id, prompt);
        let work = self.boss_work_context();
        // No spend without something to attach or a backend to judge it.
        if work.digest.is_empty() || self.resolved_eval().is_none() {
            return;
        }
        let state = crate::boss_context::router_state(prompt, &recent_prompts, &work);
        let questions = crate::boss_context::router_questions();
        let settings = self.settings.clone();
        let secrets = self.inference_secrets.clone();
        let boss = self.boss.clone();
        let agent = self.agent.clone();
        let sessions = self.sessions.clone();
        let task_state = self.task_state.clone();
        let automations = self.automations.clone();
        let _ = std::thread::Builder::new()
            .name("boss-context-router".into())
            .spawn(move || {
                let Ok(evaluation) = evaluate_with_feature(
                    &settings,
                    &secrets,
                    state,
                    questions,
                    crate::boss_context::FEATURE,
                    None,
                ) else {
                    return;
                };
                let verdict = crate::boss_context::apply_verdict(&evaluation);
                if !verdict.attach {
                    return;
                }
                // Rebuild rather than reuse the evaluated snapshot — work may
                // have moved during the call, and the attachment should
                // describe now, not the moment the prompt arrived.
                let digest = crate::boss_context::work_context(
                    &task_state.lock(),
                    &boss.document(),
                    &automations.document(),
                )
                .digest;
                if digest.is_empty() {
                    return;
                }
                let driver = sessions
                    .lock()
                    .get(&session_id)
                    .map(|entry| entry.driver.clone());
                let Some(driver) = driver else {
                    boss.router_defer_context(session_id);
                    return;
                };
                if !driver.supports_steer() || !agent.has_open_turn(session_id) {
                    boss.router_defer_context(session_id);
                    return;
                }
                // A mid-turn steer arrives as user input — frame the digest
                // as context so the provider does not read it as a new
                // instruction. Its `Blocks` tag keeps the echo out of the
                // transcript, and an accepted echo also settles the agent
                // surface — so the steer carries that block when it is owed.
                let surface = (driver.agent_surface_delivery()
                    == crate::driver::AgentSurfaceDelivery::Silent)
                    .then(|| agent.surface_block(session_id))
                    .flatten();
                let block = [
                    Some(format!(
                        "<goddard-boss-context>\n{digest}\n</goddard-boss-context>"
                    )),
                    surface,
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n\n");
                let steer = format!(
                    "Session context — background information only, not a new \
                     instruction. Continue the conversation you are already in.\n\n{block}"
                );
                agent.record_pending_steer(
                    session_id,
                    crate::agent::AgentPrompt {
                        prompt: steer.clone(),
                        transport: None,
                        sender: None,
                        queued_id: None,
                        context: Some(crate::agent::ContextSteer::Blocks),
                        hidden: false,
                        report_trigger: None,
                    },
                );
                driver.steer(steer);
            });
    }

    pub(super) fn handle_boss_operation(
        &self,
        caller: Option<Uuid>,
        operation: waku_protocol::boss::BossOperation,
        events: &EventSink,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        use waku_protocol::boss::{
            AutomationOperation, BossOperation, BossResult, EmployeeControl,
        };
        anyhow::ensure!(
            self.settings.get().boss_experiment_enabled,
            "Boss is disabled by the experiment setting"
        );
        let was_active = self.boss.is_active();
        self.boss.activate()?;
        if !was_active {
            self.wake_summon_queue();
        }
        match operation {
            BossOperation::Automation { action } => {
                if caller.is_some_and(|id| !self.boss.is_boss_principal(id)) {
                    bail!("only the boss or a human can manage automations");
                }
                use waku_protocol::automations::AutomationInput;
                let state = match action {
                    AutomationOperation::List => self.automations.document(),
                    AutomationOperation::Create { mut input } => {
                        input.id = None;
                        self.automations.upsert(input)?;
                        self.automations.document()
                    }
                    AutomationOperation::Update { input } => {
                        let id = input
                            .id
                            .ok_or_else(|| anyhow!("automation update requires an id"))?;
                        anyhow::ensure!(
                            self.automations
                                .document()
                                .automations
                                .iter()
                                .any(|automation| automation.id == id),
                            "unknown automation {id}"
                        );
                        self.automations.upsert(input)?;
                        self.automations.document()
                    }
                    AutomationOperation::Delete { automation_id } => {
                        self.automations.remove(automation_id)?;
                        self.automations.document()
                    }
                    action @ (AutomationOperation::Pause { automation_id }
                    | AutomationOperation::Resume { automation_id }) => {
                        let existing = self
                            .automations
                            .document()
                            .automations
                            .into_iter()
                            .find(|automation| automation.id == automation_id)
                            .ok_or_else(|| anyhow!("unknown automation {automation_id}"))?;
                        let resume = matches!(action, AutomationOperation::Resume { .. });
                        self.automations.upsert(AutomationInput {
                            id: Some(existing.id),
                            name: existing.name,
                            prompt: existing.prompt,
                            provider: existing.provider,
                            model: existing.model,
                            project_path: existing.project_path,
                            workspace: existing.workspace,
                            base_branch: existing.base_branch,
                            session_id: existing.session_id,
                            schedule: existing.schedule,
                            webhook: existing.webhook_secret.is_some(),
                            timezone: existing.timezone,
                            enabled: resume,
                            precheck: existing.precheck,
                            missed_run_grace_minutes: existing.missed_run_grace_minutes,
                            reuse_session: existing.reuse_session,
                        })?;
                        self.automations.document()
                    }
                };
                Ok(BossResult::Automations { state })
            }
            BossOperation::Open {
                provider,
                model,
                mode,
            } => {
                if caller.is_some() {
                    bail!("only the human can open the boss");
                }
                self.boss.with_operation_lock(|| {
                    let (identity, boss_session_id) = self.boss.identity_and_session();
                    let mut project = Project::from_path(self.boss.owned_workspace()?);
                    project.id = identity.id;
                    project.name = "Boss".into();
                    enum Step {
                        /// The row already names the boss project and its local
                        /// workspace — nothing to persist, so the reply ships
                        /// the list projection and the client keeps the detail
                        /// it holds or hydrates it through the ordinary path.
                        Ready(AgentSession),
                        /// The row's project or workspace drifted; repair it so
                        /// a stored detail blob cannot resurrect the stale
                        /// values on the next hydrate.
                        Repair(Uuid),
                        /// No boss chat yet — create it.
                        Create,
                    }
                    let step = {
                        let mut state = self.task_state.lock();
                        if let Some(existing) = state
                            .projects
                            .iter_mut()
                            .find(|entry| entry.id == project.id)
                        {
                            *existing = project.clone();
                        } else {
                            state.projects.push(project.clone());
                        }
                        match boss_session_id.and_then(|id| {
                            state
                                .sessions
                                .iter()
                                .find(|session| session.id == id)
                                .map(|session| (id, session))
                        }) {
                            Some((_, session))
                                if session.project_id == project.id
                                    && session.workspace == SessionWorkspace::default() =>
                            {
                                Step::Ready(session.list_projection())
                            }
                            Some((id, _)) => Step::Repair(id),
                            None => Step::Create,
                        }
                    };
                    match step {
                        Step::Ready(session) => Ok(BossResult::Session {
                            session: Box::new(session),
                            project: Box::new(project),
                        }),
                        Step::Repair(id) => {
                            // Drift is the rare path — hydrate on the store's
                            // own connection off the state lock, then merge
                            // and persist under it.
                            let mut session = {
                                let state = self.task_state.lock();
                                state
                                    .sessions
                                    .iter()
                                    .find(|session| session.id == id)
                                    .cloned()
                            };
                            if let Some(session) = session.as_mut() {
                                self.task_store.hydrate(session)?;
                            }
                            let mut state = self.task_state.lock();
                            let Some(existing) =
                                state.sessions.iter_mut().find(|session| session.id == id)
                            else {
                                // The row vanished mid-open — create the chat.
                                drop(state);
                                return self.create_boss_chat_session(
                                    project,
                                    identity.name,
                                    provider,
                                    model,
                                    mode,
                                );
                            };
                            if let Some(session) = session
                                && !existing.detail_loaded
                                && session.detail_loaded
                            {
                                crate::persistence::apply_session_detail(existing, session);
                            }
                            existing.project_id = project.id;
                            existing.workspace = SessionWorkspace::default();
                            let session = existing.list_projection();
                            state.mark_session_dirty(id);
                            self.task_store.save(&mut state)?;
                            Ok(BossResult::Session {
                                session: Box::new(session),
                                project: Box::new(project),
                            })
                        }
                        Step::Create => self.create_boss_chat_session(
                            project,
                            identity.name,
                            provider,
                            model,
                            mode,
                        ),
                    }
                })
            }
            BossOperation::CreatePlan {
                title,
                plan_file,
                prompt,
                provider,
                model,
                reasoning_effort,
            } => self.create_plan(
                caller,
                title,
                plan_file,
                prompt,
                provider,
                model,
                reasoning_effort,
                events,
            ),
            BossOperation::Browse { url, title } => {
                if caller.is_some_and(|id| !self.boss.is_boss_principal(id)) {
                    bail!("only the boss or a human can open browser tabs for the user");
                }
                crate::boss::validate_browse_url(&url)?;
                let session_id = caller
                    .or(self.boss.document().session_id)
                    .ok_or_else(|| anyhow!("open the boss before browsing"))?;
                events.boss_browse_requested(
                    Uuid::new_v4(),
                    session_id,
                    url.clone(),
                    title.clone(),
                );
                Ok(BossResult::Browse {
                    session_id,
                    url,
                    title,
                })
            }
            BossOperation::Terminal {
                title,
                cwd,
                command,
            } => {
                if caller.is_some_and(|id| !self.boss.is_boss_principal(id)) {
                    bail!("only the boss can create a user terminal");
                }
                let title = title.trim().to_owned();
                if title.is_empty() {
                    bail!("a terminal needs a title");
                }
                let cwd = cwd.trim().to_owned();
                if cwd.is_empty() {
                    bail!("a terminal needs a working directory");
                }
                // The intent rides the principal's own session stream —
                // `Command::Boss` requests carry no runtime id, so resolve
                // the live one or the emit drops on the runtime check.
                let session_id = caller
                    .or(self.boss.document().session_id)
                    .ok_or_else(|| anyhow!("open the boss before creating a terminal"))?;
                let runtime_id = self
                    .sessions
                    .lock()
                    .get(&session_id)
                    .map(|entry| entry.runtime_id)
                    .ok_or_else(|| anyhow!("the boss session is not running"))?;
                // The terminal belongs to the client that triggered the
                // boss's turn: a human client's own request answers on its
                // connection; an agent credential carries no subscriber, so
                // the client that prompted the session stands in. With no
                // attribution the intent broadcasts to every client, the
                // pre-routing behavior.
                let attributed = match caller {
                    Some(session) => self.boss_prompt_subscribers.lock().get(&session).copied(),
                    None => (events.source_subscriber_id() != u64::MAX)
                        .then(|| events.source_subscriber_id()),
                };
                let sink = events.for_session(session_id, runtime_id);
                let wire =
                    waku_protocol::event_to_wire(crate::model::DriverEvent::BossTerminalIntent {
                        title: title.clone(),
                        cwd: cwd.clone(),
                        command,
                    })?;
                if !attributed.is_some_and(|id| sink.send_to(id, wire.clone())) {
                    sink.send(wire)?;
                }
                Ok(BossResult::TerminalRequested { title, cwd })
            }
            BossOperation::FinalizePlan { plan_file, items } => {
                self.finalize_plan(caller, plan_file, items, events)
            }
            BossOperation::Summon {
                persona_id,
                job_title,
                prompt,
                project,
                provider,
                model,
                reasoning_effort,
                workspace,
                base_branch,
                adopt_worktree,
                permissions,
                work_goal,
                icon,
                resources,
                allow_burst,
                group_id,
                priority,
                outcome_id,
                new_outcome,
                after_success,
                finishes_outcome,
                prerequisites,
                plan,
                item,
                request_id,
            } => self.boss.with_operation_lock(|| {
                self.summon_employee(
                    caller,
                    persona_id,
                    job_title,
                    prompt,
                    project,
                    provider,
                    model,
                    reasoning_effort,
                    workspace,
                    base_branch,
                    adopt_worktree,
                    permissions,
                    work_goal,
                    icon,
                    resources,
                    allow_burst,
                    group_id,
                    priority,
                    outcome_id,
                    new_outcome,
                    after_success,
                    finishes_outcome,
                    prerequisites,
                    plan,
                    item,
                    request_id,
                    events,
                )
            }),
            BossOperation::Control { session_id, action } => {
                if matches!(&action, EmployeeControl::Prompt { .. })
                    && caller
                        .and_then(|id| self.boss.employee_including_retired(id))
                        .is_some_and(|employee| {
                            self.boss.is_boss(session_id)
                                || employee.supervisor_id == session_id
                                || self.boss.report_target(&employee) == Some(session_id)
                        })
                {
                    bail!(
                        "employees cannot prompt or steer the Boss or supervisor — use \
                         `goddard-agent boss report-blocker` only when supervisor or human action \
                         is required to proceed; otherwise report results at turn end"
                    );
                }
                self.boss.with_operation_lock(|| {
                    self.boss.require_control(caller, session_id)?;
                    // Re-tagging is pure bookkeeping — it applies in every
                    // lifecycle state, queued and finished included.
                    if let EmployeeControl::SetPlan { plan, item } = &action {
                        self.boss
                            .set_employee_plan(session_id, plan.clone(), *item)?;
                        return Ok(BossResult::Saved);
                    }
                    use waku_protocol::boss::EmployeeLifecycle;
                    match self.boss.employee_lifecycle(session_id) {
                        Some(EmployeeLifecycle::Queued) => {
                            return self.control_queued_employee(caller, session_id, action);
                        }
                        Some(EmployeeLifecycle::Dispatching) => {
                            // Mid-launch: prompts park in the mirrored queue and
                            // drain once the runtime lands; stop unwinds the
                            // in-flight grant; everything else retries against
                            // a settled state.
                            return match action {
                                EmployeeControl::Prompt { prompt, delivery } => {
                                    if prompt.trim().is_empty() {
                                        bail!("employee prompts cannot be empty");
                                    }
                                    if matches!(
                                        delivery.unwrap_or_default(),
                                        AgentPromptDelivery::Steer
                                    ) {
                                        bail!("employee is dispatching — retry once it is working");
                                    }
                                    self.queue_agent_prompt(session_id, prompt, caller, events)?;
                                    Ok(BossResult::Saved)
                                }
                                EmployeeControl::Stop => {
                                    // The cancel mark lands before the
                                    // transcript write and teardown — a
                                    // settle racing them classifies as
                                    // the intentional stop it is.
                                    self.boss.mark_cancelled(session_id)?;
                                    record_daemon_interrupt(
                                        &self.task_state,
                                        &self.task_store,
                                        session_id,
                                        Some("Cancelled during dispatch"),
                                    )?;
                                    self.finish_boss_employee(
                                        session_id,
                                        false,
                                        waku_protocol::boss::EmployeeSettle::Stopped,
                                    )?;
                                    Ok(BossResult::Saved)
                                }
                                _ => bail!("employee is dispatching — retry once it is working"),
                            };
                        }
                        Some(EmployeeLifecycle::Finishing) => {
                            bail!("employee is finishing; summon a fresh employee")
                        }
                        _ => {}
                    }
                    if let EmployeeControl::SetModel {
                        provider,
                        model,
                        reasoning_effort,
                    } = &action
                    {
                        return self.control_employee_model(
                            caller,
                            session_id,
                            *provider,
                            model,
                            reasoning_effort.clone(),
                            events,
                        );
                    }
                    if let EmployeeControl::SetPermissions { permissions } = &action {
                        self.boss.set_employee_permissions(
                            caller,
                            session_id,
                            permissions.clone(),
                        )?;
                        // The employee's next prompt re-injects its persona
                        // block so the revised grants reach it — and its
                        // memory block re-delivers with the new access line.
                        self.boss.reset_context(session_id);
                        self.agent.mark_memory_undelivered(session_id);
                        return Ok(BossResult::Saved);
                    }
                    if let EmployeeControl::SetPersona { persona_id } = &action {
                        // The deliberate replacement for an unavailable
                        // custom role — live and expired records alike.
                        self.boss.set_employee_persona(session_id, *persona_id)?;
                        self.boss.reset_context(session_id);
                        return Ok(BossResult::Saved);
                    }
                    if let EmployeeControl::SetWorkspace {
                        workspace,
                        base_branch,
                    } = &action
                    {
                        return self.control_employee_workspace(
                            caller,
                            session_id,
                            *workspace,
                            base_branch.clone(),
                            events,
                        );
                    }
                    if let EmployeeControl::SetResources { resources } = &action {
                        return self.control_employee_resources(session_id, resources.clone());
                    }
                    // A prompt or steer to a finished employee re-enters
                    // admission with the same transcript — the ticket resumes
                    // it once capacity frees; a steer has no open turn to
                    // fold into, so it becomes the next queued prompt. Stop
                    // remains live-only.
                    let was_expired = self.boss.employee(session_id).is_some_and(|e| e.expired)
                        || self
                            .boss
                            .document()
                            .retired_employees
                            .iter()
                            .any(|e| e.session_id == session_id);
                    if was_expired {
                        return match &action {
                            EmployeeControl::Prompt { prompt, .. }
                            | EmployeeControl::Steer { prompt, .. } => {
                                if prompt.trim().is_empty() {
                                    bail!("employee prompts cannot be empty");
                                }
                                // A redirecting steer may retitle the job;
                                // the label lands before requeue so the
                                // revived record already carries it.
                                if let EmployeeControl::Steer {
                                    job_title: Some(job_title),
                                    ..
                                } = &action
                                {
                                    self.boss.set_employee_job_title(session_id, job_title)?;
                                }
                                let prompt = prompt.clone();
                                self.requeue_employee(session_id, |ticket, started| {
                                    // A session that already ran replays the
                                    // prompt as a new turn behind its parked
                                    // backlog — the transcript owns the
                                    // original envelope, so the ticket's copy
                                    // goes; a never-launched shell folds the
                                    // prompt into the envelope instead.
                                    if started {
                                        ticket.prompt.clear();
                                        ticket.pending_prompts.push(prompt);
                                    } else {
                                        ticket.prompt = prompt;
                                        ticket.pending_prompts.clear();
                                    }
                                })?;
                                Ok(BossResult::Saved)
                            }
                            _ => bail!("employee has expired; prompt or steer can resume it"),
                        };
                    }
                    self.boss.require_active(session_id)?;
                    match action {
                        EmployeeControl::Prompt { prompt, delivery } => {
                            if prompt.trim().is_empty() {
                                bail!("employee prompts cannot be empty");
                            }
                            let driver = self.steerable_driver(session_id);
                            match delivery.unwrap_or_default() {
                                AgentPromptDelivery::Interrupt => match driver {
                                    Some(driver) => {
                                        self.send_agent_steer(&driver, session_id, prompt, caller)
                                    }
                                    None => {
                                        self.queue_agent_prompt(session_id, prompt, caller, events)?
                                    }
                                },
                                AgentPromptDelivery::Queue => {
                                    self.queue_agent_prompt(session_id, prompt, caller, events)?
                                }
                                AgentPromptDelivery::Steer => {
                                    let Some(driver) = driver else {
                                        bail!("employee has no steerable running turn");
                                    };
                                    self.send_agent_steer(&driver, session_id, prompt, caller);
                                }
                            }
                        }
                        EmployeeControl::Steer { prompt, job_title } => {
                            if prompt.trim().is_empty() {
                                bail!("employee prompts cannot be empty");
                            }
                            let Some(driver) = self.steerable_driver(session_id) else {
                                bail!("employee has no steerable running turn");
                            };
                            // A redirecting steer may retitle the job —
                            // bookkeeping on the roster record, applied only
                            // once the steer is deliverable; it is neither a
                            // prompt nor a transcript entry.
                            if let Some(job_title) = &job_title {
                                self.boss.set_employee_job_title(session_id, job_title)?;
                            }
                            self.send_agent_steer(&driver, session_id, prompt, caller);
                        }
                        EmployeeControl::Stop => {
                            // Same ordering as the dispatching cancel:
                            // `cancelled` is durable intent, so it lands
                            // before the transcript row and the finish.
                            self.boss.mark_cancelled(session_id)?;
                            record_daemon_interrupt(
                                &self.task_state,
                                &self.task_store,
                                session_id,
                                Some("Stopped by supervisor"),
                            )?;
                            self.finish_boss_employee(
                                session_id,
                                false,
                                waku_protocol::boss::EmployeeSettle::Stopped,
                            )?;
                        }
                        EmployeeControl::SetModel { .. }
                        | EmployeeControl::SetPermissions { .. }
                        | EmployeeControl::SetPersona { .. }
                        | EmployeeControl::SetWorkspace { .. }
                        | EmployeeControl::SetResources { .. }
                        | EmployeeControl::SetPlan { .. } => {
                            unreachable!("handled above")
                        }
                    }
                    Ok(BossResult::Saved)
                })
            }
            BossOperation::Resume { session_id } => {
                self.boss.with_operation_lock(|| {
                    // Same gate `control` uses — the human, the boss, or
                    // the employee's own supervisor decides.
                    self.boss.require_control(caller, session_id)?;
                    self.resume_employee(session_id)?;
                    Ok(BossResult::Saved)
                })
            }
            BossOperation::ReportBlocker { message } => {
                let caller =
                    caller.ok_or_else(|| anyhow!("only a Boss employee can report a blocker"))?;
                let employee = self.boss.report_blocker(caller, message)?;
                if let Some(supervisor) = self.boss.report_target(&employee) {
                    let prompt = format!(
                        "Employee {} ({}) reports a blocker that needs your attention: {}",
                        employee.identity.name,
                        employee.session_id,
                        employee.blocker.as_deref().unwrap_or_default()
                    );
                    let trigger = crate::model::ReportTrigger::new(
                        &employee,
                        crate::model::ReportTriggerKind::Blocker,
                    );
                    self.deliver_employee_report(
                        supervisor,
                        prompt,
                        employee.session_id,
                        Some(trigger),
                        events,
                    )?;
                }
                Ok(BossResult::Saved)
            }
            BossOperation::Transcript { session_id, turn } => {
                self.boss.authorize_transcript(caller, session_id)?;
                let result = self.agent_read_session(caller, Some(session_id), None, None, turn)?;
                let ResponsePayload::AgentSessionTranscript { transcript } = result else {
                    unreachable!()
                };
                Ok(BossResult::Transcript { transcript })
            }
            BossOperation::HistorySearch {
                query,
                project,
                person,
                after,
                before,
                kind,
                limit,
                offset,
            } => {
                if caller.is_some_and(|id| !self.boss.is_boss_principal(id)) {
                    bail!("only the boss or a human can search history");
                }
                // `None` scopes to the boss's reach — every retained record,
                // archives included — for the human and the boss alike.
                let result = self.history_search_scoped(
                    None,
                    &query,
                    project.as_deref(),
                    person.as_deref(),
                    after.as_deref(),
                    before.as_deref(),
                    kind,
                    limit,
                    offset,
                )?;
                Ok(BossResult::HistorySearch { result })
            }
            BossOperation::Speak { parts } => {
                let parts = self.boss.speak_parts(caller, parts)?;
                let delivered = events.speech_requested(Uuid::new_v4(), parts);
                Ok(BossResult::Speak { delivered })
            }
            BossOperation::Context => {
                if caller.is_some_and(|id| !self.boss.is_boss_principal(id)) {
                    bail!("only the boss or a human can read the work digest");
                }
                Ok(BossResult::Context {
                    context: self.boss_work_context().digest,
                })
            }
            BossOperation::Roster => {
                if caller.is_some_and(|id| !self.boss.is_boss_principal(id)) {
                    bail!("only the boss or a human can read the employee roster");
                }
                let boss = self.boss.document();
                let state = self.task_state.lock();
                Ok(BossResult::Roster {
                    roster: crate::boss_context::employee_roster(
                        &boss,
                        &state.sessions,
                        &state.projects,
                    ),
                })
            }
            BossOperation::SetResourcePolicy {
                expected_revision,
                model_limits,
                host,
            } => self.boss.with_operation_lock(|| {
                let policy =
                    self.boss
                        .set_resource_policy(caller, expected_revision, model_limits, host)?;
                if let Some(host) = &policy.host {
                    self.resource_broker()?.set_policy(host)?;
                }
                self.wake_summon_queue();
                Ok(BossResult::ResourcePolicySet { policy })
            }),
            BossOperation::SetOutcomeState {
                outcome,
                state,
                evidence,
            } => self.boss.with_operation_lock(|| {
                let (record, stop) = self
                    .boss
                    .set_outcome_state(caller, outcome, state, evidence)?;
                // A cancelled outcome stops its remaining work — the
                // cancel mark lands before teardown so the settles
                // classify as intentional, and late results stay
                // history that cannot revive the record.
                for session_id in stop {
                    self.boss.mark_cancelled(session_id)?;
                    record_daemon_interrupt(
                        &self.task_state,
                        &self.task_store,
                        session_id,
                        Some("Outcome cancelled"),
                    )?;
                    self.finish_boss_employee(
                        session_id,
                        false,
                        waku_protocol::boss::EmployeeSettle::Stopped,
                    )?;
                }
                // A reopened outcome can un-gate a waiting finishing
                // assignment — re-run dispatch either way.
                self.wake_summon_queue();
                let _ = record;
                Ok(BossResult::State {
                    state: self.boss.document(),
                })
            }),
            // A settled handoff can unblock a waiting finishing
            // assignment — re-run the dispatch pass after the decision.
            op @ BossOperation::ResolveHandoff { .. } => {
                let result = self.boss.handle(caller, op)?;
                self.wake_summon_queue();
                Ok(result)
            }
            BossOperation::SetProjectSubmissions { project, enabled } => {
                if caller.is_some_and(|id| !self.boss.is_boss_principal(id)) {
                    bail!("only the boss or a human can configure project submissions");
                }
                let mut state = self.task_state.lock();
                registered_project_mut(&mut state, &project)?.submissions_enabled = enabled;
                self.task_store.save(&mut state)?;
                drop(state);
                self.notify_task_state();
                Ok(BossResult::Saved)
            }
            BossOperation::SetProjectQaBranch { project, branch } => {
                if caller.is_some_and(|id| !self.boss.is_boss_principal(id)) {
                    bail!("only the boss or a human can configure a project's QA branch");
                }
                let branch = match branch {
                    Some(branch) if !branch.trim().is_empty() => {
                        Some(crate::review::qa_branch_checked(&branch)?)
                    }
                    _ => None,
                };
                let mut state = self.task_state.lock();
                registered_project_mut(&mut state, &project)?.qa_branch = branch;
                self.task_store.save(&mut state)?;
                drop(state);
                self.notify_task_state();
                Ok(BossResult::Saved)
            }
            BossOperation::Eval { script } => {
                if caller.is_some_and(|id| !self.boss.is_boss_principal(id)) {
                    bail!("only the boss or a human can eval boss scripts");
                }
                // The scope keys on the boss session, so a human client and
                // the boss itself share it; a replaced boss chat gets a
                // fresh one.
                let session = caller
                    .or_else(|| self.boss.document().session_id)
                    .unwrap_or_else(Uuid::nil);
                self.boss.eval(session, &script, &|operation| {
                    self.handle_boss_operation(caller, operation, events)
                })
            }
            BossOperation::Memory { operation } => {
                // Ordinary task agents — callers outside the roster — reach
                // the project bucket of the task they serve. Roster members
                // keep the service's own employee/boss resolution, and a
                // task still stamped boss-managed after the roster dropped
                // it gets neither.
                let caller_project = caller
                    .filter(|id| !self.boss.is_managed(*id))
                    .filter(|id| {
                        !self
                            .task_state
                            .lock()
                            .sessions
                            .iter()
                            .any(|session| session.id == *id && session.boss_managed)
                    })
                    .and_then(|id| self.session_memory_project(id));
                self.boss.memory(caller, caller_project, operation)
            }
            operation => {
                let rename = matches!(operation, BossOperation::Rename { .. });
                let employee_rename = match &operation {
                    BossOperation::RenameEmployee { session_id, .. } => Some(*session_id),
                    _ => None,
                };
                // Persona-text operations re-inject the changed
                // instructions into the sessions that carry them — the
                // boss chat for its own persona and each live employee
                // assigned the edited role — so the next turn (or resume,
                // or a queued ticket's dispatch) composes the current
                // saved text while a running turn keeps what it began
                // with.
                let personas_before: std::collections::HashMap<Uuid, String> = match operation {
                    BossOperation::UpsertPersona { .. } | BossOperation::PersonaDefault { .. } => {
                        self.boss
                            .document()
                            .personas
                            .iter()
                            .map(|persona| (persona.id, persona.markdown.clone()))
                            .collect()
                    }
                    _ => Default::default(),
                };
                // Recording the Employee base choice can release tickets
                // held on the unresolved marker — re-run the dispatch pass.
                let base_choice = matches!(
                    operation,
                    BossOperation::PersonaDefault {
                        action: waku_protocol::boss::PersonaDefaultAction::ChooseEmployeeBase { .. },
                    }
                );
                let result = self.boss.handle(caller, operation)?;
                if base_choice {
                    self.wake_summon_queue();
                }
                let boss = self.boss.document();
                if rename && let Some(session) = boss.session_id {
                    self.boss.reset_context(session);
                }
                if !personas_before.is_empty() {
                    for persona in &boss.personas {
                        if personas_before.get(&persona.id) == Some(&persona.markdown) {
                            continue;
                        }
                        if persona.id == boss.persona_id {
                            // The boss chat and every planning session
                            // compose the Boss persona.
                            if let Some(session) = boss.session_id {
                                self.boss.reset_context(session);
                            }
                            for plan in &boss.planning {
                                self.boss.reset_context(plan.session_id);
                            }
                        }
                        // The canonical Employee base sits beneath every
                        // employee's custom role — editing it reaches the
                        // whole roster, not just employees holding it as
                        // their selected persona.
                        let base_edit = boss.employee_persona_id == Some(persona.id);
                        // Expired employees reset too — a later resume
                        // composes the current text rather than the
                        // injection cached from its last turn.
                        for employee in boss
                            .employees
                            .iter()
                            .filter(|employee| base_edit || employee.persona_id == persona.id)
                        {
                            self.boss.reset_context(employee.session_id);
                        }
                    }
                }
                if let Some(session_id) = employee_rename {
                    // The renamed employee's next prompt re-injects its
                    // persona block so the new name reaches it.
                    self.boss.reset_context(session_id);
                }
                // A rename retitles the managed session so the sidebar and
                // top bar agree with the identity.
                let retitle = if rename {
                    boss.session_id.map(|id| (id, boss.identity.name.clone()))
                } else {
                    employee_rename.and_then(|id| {
                        boss.employees
                            .iter()
                            .find(|employee| employee.session_id == id)
                            .map(|employee| (id, employee.identity.name.clone()))
                    })
                };
                if let Some((session_id, title)) = retitle {
                    let mut state = self.task_state.lock();
                    if let Some(session) = state
                        .sessions
                        .iter_mut()
                        .find(|session| session.id == session_id)
                    {
                        session.title = title;
                        state.mark_session_dirty(session_id);
                        self.task_store.save(&mut state)?;
                    }
                }
                Ok(result)
            }
        }
    }

    /// Creates the boss's chat session and records it on the boss document.
    /// `BossOperation::Open` reaches this when the document names no session
    /// or the row it names is gone. The fresh session is returned whole —
    /// it is detail-complete the moment it exists.
    pub(super) fn create_boss_chat_session(
        &self,
        project: Project,
        title: String,
        provider: ProviderKind,
        model: Option<String>,
        mode: crate::model::RuntimeMode,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        use waku_protocol::boss::BossResult;
        let mut session = AgentSession::new(project.id, provider);
        session.model = model;
        session.runtime_mode = mode;
        session.title = title;
        session.agent_rename_allowed = false;
        session.boss_managed = true;
        let id = session.id;
        {
            let mut state = self.task_state.lock();
            state.push_session(session.clone());
            self.task_store.save(&mut state)?;
        }
        self.boss.set_session_id(id)?;
        self.wake_summon_queue();
        Ok(BossResult::Session {
            session: Box::new(session),
            project: Box::new(project),
        })
    }

    /// `drain` yields while the session's provider turn is still open —
    /// the settle path passes it so a turn that began between the settle
    /// event and this pass runs to its own boundary, and that settle
    /// re-drives the finish. Stops, launch failures, and restart recovery
    /// pass `false`: they tear a live turn down on purpose. `settle`
    /// reports how the admission ended so the expired record can carry a
    /// legible cause rather than a bare dead row.
    pub(crate) fn finish_boss_employee(
        &self,
        session_id: Uuid,
        drain: bool,
        settle: waku_protocol::boss::EmployeeSettle,
    ) -> anyhow::Result<()> {
        // `finishing` persists while the teardown runs — the model slot
        // stays claimed through shutdown, so a crash mid-finish leaves a
        // reconciling record instead of a leaked or double-counted slot.
        // The session's terminal verdict feeds the wave tally — read it
        // before the durable transition commits the member's outcome.
        let failed = self
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .is_some_and(|session| session.status == SessionStatus::Failed);
        // Leftovers are unknown until the tail reads the session document
        // — the preliminary classification here is what a restart would
        // recover if the tail never ran.
        let cause = waku_protocol::boss::ExpiryCause::for_settle(settle, failed, 0, false);
        let Some(employee) = self
            .boss
            .begin_finishing(session_id, failed, drain, cause)?
        else {
            return Ok(());
        };
        self.finish_boss_employee_tail(session_id, &employee, settle)
    }

    /// The teardown after `begin_finishing` — also driven directly by
    /// restart recovery for records already parked at `finishing`. The
    /// steps tolerate a resumed run: teardown no-ops on a missing
    /// runtime, `complete_expiry` takes the reservation exactly once,
    /// and the report goes out only while the record can still be read
    /// as finishing.
    pub(super) fn finish_boss_employee_tail(
        &self,
        session_id: Uuid,
        employee: &waku_protocol::boss::BossEmployee,
        settle: waku_protocol::boss::EmployeeSettle,
    ) -> anyhow::Result<()> {
        self.integrations.revoke_task(session_id);
        // An unanswered ask's question outlives its waiter — capture it
        // before session teardown drains it as cancelled.
        let pending_question = self.agent.pending_ask_question(session_id);
        self.agent.clear_session(session_id);
        let removed = self.sessions.lock().remove(&session_id);
        if let Some(entry) = &removed {
            entry.driver.begin_shutdown();
            let sink = self
                .event_source
                .lock()
                .for_session(session_id, entry.runtime_id);
            // Attached clients keep their driver handle until the
            // runtime-ended signal lands; `end_session_runtime` alone
            // strands it — the same contract idle eviction follows — and
            // a stale handle pins the sidebar's last-known status, so a
            // dead employee keeps showing working forever.
            sink.notify_runtime_ended();
            sink.end_session_runtime();
        }
        drop_detached(removed);
        let (body, failed, parked_prompts) = {
            let mut state = self.task_state.lock();
            let mut body = String::new();
            let mut failed = false;
            let mut parked = 0_u32;
            // A session gone from the document still expires — the record
            // keeps the settle's classification rather than stranding at
            // `finishing` on a missing transcript.
            if let Some(index) = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
            {
                self.task_store.hydrate(&mut state.sessions[index])?;
                let session = &mut state.sessions[index];
                if session.active_turn_id().is_some() {
                    session.interrupt_active_turn(crate::model::TurnInterruption::Daemon);
                }
                failed = session.status == SessionStatus::Failed;
                if !failed {
                    session.status = SessionStatus::Idle;
                }
                // Daemon-owned parked prompts survive the expiry: the
                // mirrored queue is the revive path's backlog, so a
                // follow-up sent while the employee wound down still
                // reaches its next turn. The count lands on the settle
                // record.
                parked = session
                    .queued_messages
                    .iter()
                    .filter(|queued| queued.is_agent_owned())
                    .count() as u32;
                session
                    .queued_messages
                    .retain(|queued| queued.is_agent_owned());
                // The report carries a pointer index; finishing does not
                // record any of its claims as durable memory.
                let (rendered, _) =
                    crate::model::render_transcript_index(&session.transcript_index());
                body = rendered.trim_end().to_owned();
                state.mark_session_dirty(session_id);
                self.task_store.save(&mut state)?;
            }
            (body, failed, parked)
        };
        // `begin_finishing` recorded the preliminary cause — a `finishing`
        // record recovered after a restart keeps it — so the tail only
        // fills leftovers and upgrades a clean settle whose queue or ask
        // outlived the turn.
        let cause = employee
            .expiry
            .as_ref()
            .map(|expiry| expiry.cause)
            .unwrap_or_else(|| {
                waku_protocol::boss::ExpiryCause::for_settle(
                    settle,
                    failed,
                    parked_prompts,
                    pending_question.is_some(),
                )
            });
        let cause = match (cause, pending_question.is_some(), parked_prompts > 0) {
            (waku_protocol::boss::ExpiryCause::Finished, true, _) => {
                waku_protocol::boss::ExpiryCause::UnansweredAsk
            }
            (waku_protocol::boss::ExpiryCause::Finished, false, true) => {
                waku_protocol::boss::ExpiryCause::ParkedWork
            }
            (cause, ..) => cause,
        };
        // The settle that ended the admission reads honestly in the
        // transcript — a restart or provider exit is nobody's "you
        // stopped". A recovered `finishing` rerun skips a row the first
        // pass already wrote.
        if let Some(text) = cause.notice() {
            let mut state = self.task_state.lock();
            if let Some(session) = state
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
                && self.task_store.hydrate(session).is_ok()
                && !session.messages.iter().any(|message| {
                    matches!(
                        &message.notice,
                        Some(crate::model::TranscriptNotice::Status {
                            kind: crate::model::TranscriptNoticeStatus::Interrupted,
                        })
                    )
                })
            {
                session.push_notice_message(
                    crate::model::MessageRole::Assistant,
                    text,
                    crate::model::TranscriptNotice::Status {
                        kind: crate::model::TranscriptNoticeStatus::Interrupted,
                    },
                );
                state.mark_session_dirty(session_id);
                self.task_store.save(&mut state)?;
            }
        }
        let expiry = waku_protocol::boss::EmployeeExpiry {
            cause,
            resumable: cause.resumable(),
            parked_prompts,
            pending_question,
        };
        // Capacity releases exactly once, after the runtime is gone —
        // `complete_expiry` hands back the ticket's reservations only on
        // the winning expiry, and the broker's release keeps resident
        // devices under their own retention rules. Waking the scheduler
        // is what pulls the next queued ticket forward.
        for reservation in self
            .boss
            .complete_expiry(session_id, Some(expiry.clone()))?
        {
            if let Ok(broker) = self.resource_broker() {
                broker.release_admission(session_id, reservation);
            }
        }
        self.wake_summon_queue();
        eprintln!(
            "employee {session_id} expired — cause {}, resumable {}, parked {}, pending question {}",
            expiry.cause.label(),
            expiry.resumable,
            expiry.parked_prompts,
            expiry.pending_question.is_some()
        );
        // Re-read the expired record — it now carries the refined settle
        // detail (cause, leftovers, resumability) the report keys on.
        let employee = self
            .boss
            .employee(session_id)
            .unwrap_or_else(|| employee.clone());
        let expiry = employee.expiry.as_ref();
        let interrupted = expiry.is_some_and(|expiry| expiry.reports());
        // The work kind the summon fixed decides whether a clean finish
        // reports: an errand's lands with the supervisor (escalating to
        // the boss when the supervisor cannot take prompts), while a
        // unlinked goal's stays silent — the record lists on the client's
        // Goals page instead. Approved plan work always wakes its supervisor
        // with the durable queue so it can review and advance the chain.
        // An interruption reports for either kind, and so does a settle
        // that left prompts parked or an ask unanswered.
        // A cancelled record expired on a supervisor's stop — intentional,
        // already visible on the record, and not a failure. It never wakes
        // the supervisor: whoever stopped it already knows.
        let assignment_finish = self.boss.assignment_finished(&employee)?;
        let continuation = self.boss.plan_continuation_context(&employee);
        // A linked assignment's success semantics sit above the work-kind
        // switch: an ordinary success always hands off with its intent, a
        // completion conflict always reports, and an accepted finish is
        // silent even when the kind or settings would report.
        let reports = !employee.cancelled
            && !matches!(assignment_finish, crate::boss::AssignmentFinish::Completed)
            && (matches!(
                assignment_finish,
                crate::boss::AssignmentFinish::Handoff { .. }
                    | crate::boss::AssignmentFinish::Conflict { .. }
            ) || employee.work_goal == waku_protocol::boss::EmployeeGoal::Errand
                || continuation.is_some()
                || employee.blocker.is_some()
                || failed
                || interrupted);
        if reports && let Some(supervisor) = self.boss.report_target(&employee) {
            use waku_protocol::boss::ExpiryCause;
            let cause = expiry.map(|expiry| expiry.cause);
            // A repeat interruption names its ordinal — a crash loop
            // reads "interruption #3" rather than another first failure.
            let ordinal = employee
                .ticket
                .as_ref()
                .map(|ticket| ticket.interruptions.len())
                .filter(|count| *count > 1)
                .map(|count| format!(" This is interruption #{count} on its ticket."))
                .unwrap_or_default();
            let detail = match cause {
                Some(ExpiryCause::Restarted) => {
                    "was interrupted by a daemon restart and expired".to_owned()
                }
                Some(ExpiryCause::ExitedMidTurn) => {
                    "was interrupted — the provider process exited mid-turn — and expired"
                        .to_owned()
                }
                Some(ExpiryCause::ExitedIdle) => {
                    "expired — the provider process exited while it was idle".to_owned()
                }
                Some(ExpiryCause::Failed) => "expired with a failed turn".to_owned(),
                Some(ExpiryCause::ParkedWork) => format!(
                    "expired with {} parked prompt{} that never delivered",
                    expiry.map(|expiry| expiry.parked_prompts).unwrap_or(0),
                    if expiry.is_some_and(|expiry| expiry.parked_prompts == 1) {
                        ""
                    } else {
                        "s"
                    }
                ),
                Some(ExpiryCause::UnansweredAsk) => {
                    "expired with an unanswered question for the user".to_owned()
                }
                Some(ExpiryCause::Stopped) => "was stopped and expired".to_owned(),
                _ => "has finished and expired".to_owned(),
            };
            let blocker = match employee.blocker.as_deref() {
                Some(note) if self.employee_blocker_reported(supervisor, session_id, note)? => {
                    " It flagged a blocker.".to_owned()
                }
                Some(note) => format!(" It flagged a blocker: {note}"),
                None => String::new(),
            };
            let mut prompt = format!(
                "Employee {} ({session_id}) {detail}.{ordinal}{blocker}",
                employee.identity.name
            );
            if let Some(expiry) = expiry {
                if let Some(question) = &expiry.pending_question {
                    prompt.push_str(&format!(" Its unanswered question: \"{question}\""));
                }
                if expiry.parked_prompts > 0 && expiry.cause != ExpiryCause::ParkedWork {
                    prompt.push_str(&format!(
                        " {} parked prompt{} survived the expiry and drain first if it resumes.",
                        expiry.parked_prompts,
                        if expiry.parked_prompts == 1 { "" } else { "s" }
                    ));
                }
                if interrupted && expiry.resumable {
                    prompt.push_str(&format!(
                        " It is resumable — goddard-agent boss '{{\"type\":\"resume\",\"sessionId\":\"{session_id}\"}}' revives it in place with its transcript and workspace intact."
                    ));
                }
            }
            prompt.push_str(&format!(
                " Its transcript index follows. Read relevant turns using goddard-agent boss '{{\"type\":\"transcript\",\"sessionId\":\"{session_id}\",\"turn\":N}}'.\n\n{body}"
            ));
            match &assignment_finish {
                crate::boss::AssignmentFinish::Handoff { id, intent } => {
                    let outcome_id = employee
                        .assignment
                        .as_ref()
                        .map(|assignment| assignment.outcome_id)
                        .unwrap_or_default();
                    prompt.push_str(&format!(
                        "\n\nOutcome handoff {id} is pending — the assignment served outcome {outcome_id}. Planned after success: \"{intent}\" Resolve it explicitly with goddard-agent boss '{{\"type\":\"resolveHandoff\",\"outcome\":\"{outcome_id}\",\"handoff\":\"{id}\",\"decision\":{{\"type\":\"assign\",\"assignment\":\"<employee sessionId>\"}} | {{\"type\":\"dismiss\"}} | {{\"type\":\"completeOutcome\",\"evidence\":\"...\"}}}}' — reading this report does not settle it."
                    ));
                }
                crate::boss::AssignmentFinish::Conflict { reason } => {
                    let outcome_id = employee
                        .assignment
                        .as_ref()
                        .map(|assignment| assignment.outcome_id)
                        .unwrap_or_default();
                    prompt.push_str(&format!(
                        "\n\nIt was the finishing assignment for outcome {outcome_id}, but completion was refused: {reason}. The outcome is unfinished — resolve the outstanding items, then complete it explicitly or assign a new finishing assignment."
                    ));
                }
                _ if employee
                    .assignment
                    .as_ref()
                    .is_some_and(|assignment| !assignment.finishes_outcome) =>
                {
                    let intent = employee
                        .assignment
                        .as_ref()
                        .map(|assignment| assignment.after_success.as_str())
                        .unwrap_or_default();
                    prompt.push_str(&format!("\n\nPlanned after success: \"{intent}\""));
                }
                _ => {}
            }
            if let Some(continuation) = continuation {
                prompt.push_str(&continuation);
            }
            // The marker carries the settle's verdict: interruption-class
            // causes mark `interrupted`; failure still outranks a flagged
            // blocker when the settle itself was clean.
            let kind = match cause {
                Some(
                    ExpiryCause::Restarted
                    | ExpiryCause::ExitedMidTurn
                    | ExpiryCause::ExitedIdle
                    | ExpiryCause::ParkedWork
                    | ExpiryCause::UnansweredAsk,
                ) => crate::model::ReportTriggerKind::Interrupted,
                _ if failed || cause == Some(ExpiryCause::Failed) => {
                    crate::model::ReportTriggerKind::Failed
                }
                _ if employee.blocker.is_some() => {
                    crate::model::ReportTriggerKind::FinishedWithBlocker
                }
                _ => crate::model::ReportTriggerKind::Finished,
            };
            let trigger = crate::model::ReportTrigger::new(&employee, kind);
            let events = self.event_source.lock().clone();
            if interrupted {
                // Interruption reports take the blocker path — they reach
                // a busy supervisor mid-turn rather than queueing behind
                // its work.
                self.deliver_employee_report(
                    supervisor,
                    prompt,
                    session_id,
                    Some(trigger),
                    &events,
                )?;
            } else {
                self.queue_agent_prompt_with_id(
                    supervisor,
                    prompt,
                    Some(session_id),
                    true,
                    None,
                    Some(trigger),
                    &events,
                )?;
            }
        }
        // This finish may have resolved a wave — drain its notice.
        self.deliver_wave_notifications();
        Ok(())
    }

    /// The report may be in the transcript, parked durably, or awaiting a
    /// steer's echo. Reuse those delivery records so expiry does not quote
    /// the same blocker again, including after a daemon restart.
    fn employee_blocker_reported(
        &self,
        supervisor: Uuid,
        employee: Uuid,
        blocker: &str,
    ) -> anyhow::Result<bool> {
        use crate::model::{ReportTrigger, ReportTriggerKind};
        let normalize = |text: &str| {
            text.trim()
                .trim_end_matches(['.', '!', '?'])
                .split_whitespace()
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let blocker = normalize(blocker);
        let matches = |content: &str, trigger: Option<&ReportTrigger>| {
            trigger.is_some_and(|trigger| {
                trigger.employee == employee && trigger.kind == ReportTriggerKind::Blocker
            }) && content
                .split_once("reports a blocker that needs your attention: ")
                .is_some_and(|(_, report)| normalize(report) == blocker)
        };
        if self.agent.has_pending_steer_matching(supervisor, |steer| {
            matches(&steer.prompt, steer.report_trigger.as_ref())
        }) {
            return Ok(true);
        }
        let mut state = self.task_state.lock();
        let Some(session) = state
            .sessions
            .iter_mut()
            .find(|session| session.id == supervisor)
        else {
            return Ok(false);
        };
        self.task_store.hydrate(session)?;
        Ok(session
            .messages
            .iter()
            .any(|message| matches(&message.content, message.report_trigger.as_ref()))
            || session
                .queued_messages
                .iter()
                .any(|queued| matches(&queued.content, queued.report_trigger.as_ref())))
    }

    /// `control`'s `setWorkspace` action: one operation that stops the
    /// employee's running turn, rebinds the session — `local` returns it
    /// to the project's primary checkout, `worktree` forks a fresh
    /// daemon-managed worktree off `base_branch` — and resumes the same
    /// transcript there. Every fallible step runs before the turn is
    /// touched, so a failure leaves the employee running in its old
    /// workspace rather than stopped between the two.
    pub(super) fn control_employee_workspace(
        &self,
        caller: Option<Uuid>,
        session_id: Uuid,
        workspace: AgentWorkspace,
        base_branch: Option<String>,
        events: &EventSink,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        use waku_protocol::boss::BossResult;
        self.boss.require_active(session_id)?;
        if self.boss.employee(session_id).is_none() {
            bail!("a retired employee's workspace changes when it is revived");
        }
        let (project_path, old_path) = {
            let state = self.task_state.lock();
            let session = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("employee session is missing"))?;
            if matches!(workspace, AgentWorkspace::Local) && session.workspace.is_local() {
                bail!("employee already runs in the project's primary checkout");
            }
            let project_path = state
                .projects
                .iter()
                .find(|project| project.id == session.project_id)
                .map(|project| project.path.clone())
                .ok_or_else(|| anyhow!("employee session has no project to switch within"))?;
            let old_path = session
                .workspace
                .path()
                .unwrap_or(&project_path)
                .to_path_buf();
            (project_path, old_path)
        };
        if matches!(workspace, AgentWorkspace::Adopt) {
            bail!(
                "adopting a worktree is summon-only — setWorkspace accepts \"local\" or \"worktree\""
            );
        }
        let base_branch = base_branch
            .map(|branch| branch.trim().to_owned())
            .filter(|branch| !branch.is_empty());
        if matches!(workspace, AgentWorkspace::Worktree) && base_branch.is_none() {
            bail!("worktree workspaces require a base branch");
        }
        // Creation runs before the turn is touched so a Git failure leaves
        // the employee running in its old workspace.
        let created = match workspace {
            AgentWorkspace::Local => None,
            AgentWorkspace::Worktree => Some(crate::worktree::create(
                &project_path,
                None,
                base_branch.as_deref(),
                false,
                &[],
            )?),
            AgentWorkspace::Adopt => unreachable!("adopt is rejected above"),
        };
        // Persist the settle guard before cancellation can emit its close
        // event. The employee keeps its admission and live sidebar row;
        // startup recovery interrupts an abandoned move instead of leaving
        // a falsely expired record behind.
        if let Err(error) = self.boss.set_workspace_transition(session_id, true) {
            if let Some(worktree) = &created {
                let _ = crate::worktree::remove(&worktree.path, true);
            }
            return Err(error);
        }
        let switch = (|| -> anyhow::Result<()> {
            if self.agent.has_open_turn(session_id)
                && let Some(entry) = self.sessions.lock().get(&session_id)
            {
                entry.driver.cancel();
            }
            let removed = self.sessions.lock().remove(&session_id);
            if let Some(entry) = &removed {
                entry.driver.begin_shutdown();
            }
            drop_detached(removed);
            self.agent.revoke_session(session_id);
            // Clearing the guard before the cancelled turn's close event lands
            // would let its settle finish the employee for real — the
            // forwarder reads the guard at event time, so the open-turn flag
            // clearing is what proves that evaluation already happened.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while self.agent.has_open_turn(session_id) {
                if std::time::Instant::now() >= deadline {
                    bail!("could not stop the employee's current turn");
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            let mut state = self.task_state.lock();
            let session = state
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("employee session is missing"))?;
            self.task_store.hydrate(session)?;
            let to = created
                .as_ref()
                .map(|worktree| worktree.path.clone())
                .unwrap_or_else(|| project_path.clone());
            session.workspace = match &created {
                Some(worktree) => SessionWorkspace::Worktree {
                    path: worktree.path.clone(),
                    name: worktree.name.clone(),
                    branch: None,
                    base_branch: base_branch.clone(),
                    adopted_by: None,
                },
                None => SessionWorkspace::Local,
            };
            // The next delivered prompt folds this in, the same one-shot
            // channel a provider switch uses — the transcript keeps the
            // employee's own messages verbatim.
            session.pending_provider_context = Some(format!(
                "Your supervisor moved this session to a different workspace — \
                 your working directory is now {}. Treat it as the project \
                 root: read and write files only under it. The checkout it ran \
                 in before, {}, still exists — absolute paths recorded earlier \
                 in this conversation point there, and anything you left \
                 uncommitted stayed behind.",
                to.display(),
                old_path.display()
            ));
            session.updated_at = crate::model::unix_time();
            state.mark_session_dirty(session_id);
            self.task_store.save(&mut state)?;
            Ok(())
        })();
        if let Err(error) = switch {
            let restored = self.boss.set_workspace_transition(session_id, false);
            // The fork only ever held a fresh checkout — nothing the
            // employee wrote — so abandoning it on a failed switch loses
            // nothing.
            if let Some(worktree) = &created {
                let _ = crate::worktree::remove(&worktree.path, true);
            }
            restored?;
            return Err(error);
        }
        self.boss.set_workspace_transition(session_id, false)?;
        self.queue_agent_prompt_hidden(
            session_id,
            "Continue the job you were assigned.".to_owned(),
            caller,
            events,
        )
        .context("the workspace switched, but the employee could not be resumed")?;
        Ok(BossResult::Saved)
    }
}
