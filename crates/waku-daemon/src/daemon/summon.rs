use super::*;

impl WakuBackend {
    // ---- Summon queue: admission, dispatch, recovery ----

    /// Spawn the summon scheduler once — the worker every queue wake and
    /// the bounded reconciliation tick share. Nothing here holds
    /// `operation_lock`; admission decisions serialize through
    /// `BossService::update` and the broker's authority lock.
    pub(super) fn start_summon_scheduler(self: &Arc<Self>) {
        use std::sync::atomic::Ordering;
        if !self.boss.is_active() || self.summon_scheduler_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let backend = Arc::downgrade(self);
        let wake = self.summon_wake.clone();
        let _ = std::thread::Builder::new()
            .name("summon-scheduler".into())
            .spawn(move || {
                loop {
                    let Some(backend) = backend.upgrade() else {
                        return;
                    };
                    backend.run_summon_scheduler();
                    let queued = !backend.boss.queued_heads().is_empty()
                        || !backend.boss.pending_resource_updates().is_empty();
                    let (lock, condvar) = &*wake;
                    let mut signaled = lock.lock();
                    // Queued tickets get a bounded tick so broker-side changes
                    // no daemon event announces — a freed device, an external
                    // release — still reach the queue. An idle queue parks.
                    let timeout = if queued {
                        SUMMON_RECONCILE_INTERVAL
                    } else {
                        std::time::Duration::from_secs(3600)
                    };
                    // A wake that landed mid-pass already set the flag —
                    // honoring it before parking keeps the queue prompt.
                    if !*signaled {
                        condvar.wait_for(&mut signaled, timeout);
                    }
                    *signaled = false;
                }
            });
    }

    /// The host resource broker — or the test-rooted ledger a backend was
    /// pointed at.
    pub(super) fn resource_broker(&self) -> anyhow::Result<crate::resource_broker::Broker> {
        match self.broker_root.lock().clone() {
            Some(root) => Ok(crate::resource_broker::Broker::at(root)),
            None => crate::resource_broker::Broker::host(),
        }
    }

    /// Wake the scheduler for one dispatch pass — call after enqueueing a
    /// ticket, releasing a slot, or changing policy or a queued selection.
    /// Without a scheduler thread (as in tests before startup) the pass
    /// runs inline, so admissions still land before the op returns.
    pub(super) fn wake_summon_queue(&self) {
        if self
            .summon_scheduler_started
            .load(std::sync::atomic::Ordering::Acquire)
        {
            let (lock, condvar) = &*self.summon_wake;
            *lock.lock() = true;
            condvar.notify_all();
        } else {
            self.run_summon_scheduler();
        }
    }

    /// One full dispatch pass: every queued ticket attempts an atomic
    /// admission grant in priority/sequence order. A denied ticket records
    /// its wait reason and is skipped; later tickets still get an attempt.
    /// Repeat while progress is made so a released slot can admit another
    /// ticket without waiting for the next wakeup.
    pub(crate) fn run_summon_scheduler(&self) {
        // The boss document owns the policy; the broker's file follows it
        // when one was set — a crash between the two writes reconciles
        // here rather than dispatching under a stale host policy.
        if let Some(host) = self.boss.document().resource_policy.host
            && let Ok(broker) = self.resource_broker()
            && broker.policy().is_ok_and(|current| current != host)
            && let Err(error) = broker.set_policy(&host)
        {
            eprintln!("could not reconcile host resource policy: {error:#}");
        }
        self.deliver_dispatch_notifications();
        self.deliver_wave_notifications();
        loop {
            let heads = self.boss.queued_heads();
            let updates = self.boss.pending_resource_updates();
            if heads.is_empty() && updates.is_empty() {
                return;
            }
            let mut progressed = false;
            for employee in heads {
                match self.dispatch_queued_head(&employee) {
                    Ok(true) => progressed = true,
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!(
                            "summon dispatch for {} failed: {error:#}",
                            employee.session_id
                        );
                    }
                }
            }
            // Parked resource updates attempt after queued tickets — new
            // work sees capacity first; a granted update swaps in place
            // and its release may admit another ticket next round.
            for employee in updates {
                match self.try_resource_update(&employee) {
                    Ok(true) => progressed = true,
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!(
                            "resource update for {} failed: {error:#}",
                            employee.session_id
                        );
                    }
                }
            }
            if !progressed {
                return;
            }
        }
    }

    /// One parked resource update's admission try: the broker grants the
    /// new set under the parked id only when capacity outside the
    /// employee's own claims allows, then the ticket swaps to it
    /// atomically and the old claims release. A denied or stale attempt
    /// changes nothing — the employee keeps running on its current set.
    pub(super) fn try_resource_update(
        &self,
        employee: &waku_protocol::boss::BossEmployee,
    ) -> anyhow::Result<bool> {
        let session_id = employee.session_id;
        let Some(ticket) = employee.ticket.clone() else {
            return Ok(false);
        };
        let (Some(resources), Some(reservation_id)) =
            (ticket.pending_resources.clone(), ticket.pending_reservation)
        else {
            return Ok(false);
        };
        let attempt = self.resource_broker().and_then(|broker| {
            broker.try_admission(
                session_id,
                reservation_id,
                resources,
                employee.job_title.clone(),
                self.admission_claim(&ticket),
            )
        });
        let attempt = match attempt {
            Ok(attempt) => attempt,
            Err(error) => {
                self.boss.record_blocked(
                    session_id,
                    vec![waku_protocol::boss::AdmissionBlocker::HostResources {
                        detail: format!("{error:#}"),
                    }],
                )?;
                return Ok(false);
            }
        };
        if !attempt.granted {
            self.boss.record_blocked(session_id, attempt.blockers)?;
            return Ok(false);
        }
        match self
            .boss
            .apply_resource_update(session_id, reservation_id)?
        {
            (true, stale_reservation) => {
                if let Some(stale) = stale_reservation
                    && let Ok(broker) = self.resource_broker()
                {
                    broker.release_admission(session_id, stale);
                }
                // The launch-time shim bakes the reservation id — rewrite
                // it so the employee's next `resource` call borrows from
                // the new set. Provider processes that carry the id in
                // their own environment keep the old one until relaunch.
                match self
                    .agent_launch_env(session_id)
                    .and_then(|env| crate::agent::write_session_shim(&env))
                {
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("could not refresh the session shim for {session_id}: {error:#}");
                    }
                }
                Ok(true)
            }
            (false, _) => {
                // Superseded, requeued, or finished while the grant was in
                // flight — drop the claims it just took.
                if let Ok(broker) = self.resource_broker() {
                    broker.release_admission(session_id, reservation_id);
                }
                Ok(false)
            }
        }
    }

    /// The model-slot claim one admission attempt carries — the ticket's
    /// provider and concrete model against the boss-set limits, burst
    /// allowed only when the ticket says so.
    pub(super) fn admission_claim(
        &self,
        ticket: &waku_protocol::boss::SummonTicket,
    ) -> waku_protocol::resources::AdmissionClaim {
        let rule = self.boss.model_limit(ticket.provider, &ticket.model);
        waku_protocol::resources::AdmissionClaim {
            daemon: self.boss.document().identity.id,
            provider: ticket.provider.id().to_owned(),
            model: ticket.model.clone(),
            live_limit: rule
                .as_ref()
                .map(|rule| rule.live_limit)
                .unwrap_or(u32::MAX),
            hard_cap: rule.as_ref().map(|rule| rule.hard_cap).unwrap_or(u32::MAX),
            allow_burst: ticket.allow_burst,
        }
    }

    /// Try one head-of-line ticket: atomic model+resource grant through
    /// the broker, then the persisted launch outside every lock. A grant
    /// the state transition rejects is released immediately — stale
    /// generation, cancelled ticket.
    pub(super) fn dispatch_queued_head(
        &self,
        employee: &waku_protocol::boss::BossEmployee,
    ) -> anyhow::Result<bool> {
        let Some(ticket) = employee.ticket.clone() else {
            return Ok(false);
        };
        let session_id = employee.session_id;
        let claim = self.admission_claim(&ticket);
        // The reservation key is stable per generation, so a retry after a
        // lost response or a restart re-issues instead of double-claiming.
        let reservation_id = Uuid::from_u128(session_id.as_u128() ^ u128::from(ticket.generation));
        let attempt = self.resource_broker().and_then(|broker| {
            broker.try_admission(
                session_id,
                reservation_id,
                ticket.resources.clone(),
                employee.job_title.clone(),
                claim,
            )
        });
        let attempt = match attempt {
            Ok(attempt) => attempt,
            Err(error) => {
                // Validation-level failures mean the accepted set became
                // impossible — hold the ticket pending with the reason
                // until it is edited, canceled, or policy changes.
                self.boss.record_blocked(
                    session_id,
                    vec![waku_protocol::boss::AdmissionBlocker::HostResources {
                        detail: format!("{error:#}"),
                    }],
                )?;
                return Ok(false);
            }
        };
        if !attempt.granted {
            self.boss.record_blocked(session_id, attempt.blockers)?;
            return Ok(false);
        }
        if !self
            .boss
            .mark_dispatching(session_id, ticket.generation, Some(reservation_id))?
        {
            // A stop or re-admission settled the generation mid-grant —
            // drop the claims; nobody else will.
            if let Ok(broker) = self.resource_broker() {
                broker.release_admission(session_id, reservation_id);
            }
            return Ok(false);
        }
        match self.dispatch_employee(employee, &ticket) {
            Ok(resumed) => {
                if !self.boss.mark_working(session_id, ticket.generation)? {
                    // Stopped or re-admitted while the launch ran — the
                    // finish path already reclaimed the slot; tear down
                    // the runtime this launch just started.
                    let removed = self.sessions.lock().remove(&session_id);
                    if let Some(entry) = &removed {
                        entry.driver.begin_shutdown();
                    }
                    drop_detached(removed);
                    return Ok(false);
                }
                if resumed {
                    // The envelope parked in the daemon queue while the
                    // ticket was dispatching — drain it in order now that
                    // the record reads working. Delivery events belong to
                    // the session's runtime sink: emitted on the root source
                    // they target `(nil, nil)`, fail the hub's active-runtime
                    // check, and never reach the journal or attached clients.
                    let events = self.event_source.lock().clone();
                    let (runtime_id, driver) = self.ensure_agent_runtime(session_id, &events)?;
                    let sink = events.for_session(session_id, runtime_id);
                    self.drain_agent_queue(session_id, &driver, &sink)?;
                }
                // The envelope has been adopted — the transcript owns it
                // now, and a later requeue must not replay it.
                let _ = self.boss.clear_employee_prompt_queue(session_id);
                self.boss.outbox_push(
                    session_id,
                    ticket.generation,
                    ticket.provider,
                    ticket.model.clone(),
                    ticket.goal_id,
                )?;
                self.deliver_dispatch_notifications();
                Ok(true)
            }
            Err(error) => {
                let _ = record_boss_event(
                    &self.task_state,
                    &self.task_store,
                    session_id,
                    &DriverEvent::Error(format!("Employee launch failed: {error:#}")),
                );
                // `dispatching -> expired`: the blocker rides the finish
                // report so the supervisor hears why the job died, and the
                // summon RPC surfaces an immediate failure as an error.
                let _ = self
                    .boss
                    .set_employee_blocker(session_id, format!("Employee launch failed: {error:#}"));
                self.finish_boss_employee(
                    session_id,
                    false,
                    waku_protocol::boss::EmployeeSettle::LaunchFailed,
                )?;
                Ok(true)
            }
        }
    }

    /// The persisted launch for a granted ticket. Fresh summons adopt the
    /// envelope — assignment plus every prompt parked while queued — onto
    /// the shell and start the provider; re-admitted employees (revival,
    /// setModel) park their ticket prompts so the caller can drain them in
    /// order once `mark_working` lands. Deferred launch revalidates the
    /// mutable parts: the project path and the worktree fork happen now,
    /// not at admission. Returns `true` when the session resumed rather
    /// than cold-started.
    pub(super) fn dispatch_employee(
        &self,
        employee: &waku_protocol::boss::BossEmployee,
        ticket: &waku_protocol::boss::SummonTicket,
    ) -> anyhow::Result<bool> {
        let session_id = employee.session_id;
        let events = self.event_source.lock().clone();
        let started = {
            let mut state = self.task_state.lock();
            let index = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("employee session is missing"))?;
            self.task_store.hydrate(&mut state.sessions[index])?;
            state.sessions[index].has_started()
        };
        if started {
            for prompt in std::iter::once(ticket.prompt.clone())
                .chain(ticket.pending_prompts.iter().cloned())
                .filter(|prompt| !prompt.trim().is_empty())
            {
                self.queue_agent_prompt(session_id, prompt, Some(employee.supervisor_id), &events)?;
            }
            return Ok(true);
        }
        let project = dunce::canonicalize(Path::new(&ticket.project))
            .with_context(|| format!("project path {} does not exist", ticket.project))?;
        let (project_id, project_path) = self.register_agent_project(&project)?;
        let (workspace, adopted_from) = match ticket.workspace.unwrap_or_default() {
            AgentWorkspace::Local => (SessionWorkspace::Local, None),
            AgentWorkspace::Worktree => {
                let created = crate::worktree::create(
                    &project_path,
                    None,
                    ticket.base_branch.as_deref(),
                    false,
                    &[],
                )?;
                (
                    SessionWorkspace::Worktree {
                        path: created.path,
                        name: created.name,
                        branch: None,
                        base_branch: ticket.base_branch.clone(),
                        adopted_by: None,
                    },
                    None,
                )
            }
            AgentWorkspace::Adopt => {
                let adopt = ticket
                    .adopt_worktree
                    .as_deref()
                    .ok_or_else(|| anyhow!("adopt workspaces require an adoptWorktree path"))?;
                let adoption =
                    self.resolve_worktree_adoption(&project_path, adopt, Some(session_id))?;
                (adoption.workspace, Some(adoption.owner_name))
            }
        };
        let mut envelope = std::iter::once(ticket.prompt.clone())
            .chain(ticket.pending_prompts.iter().cloned())
            .collect::<Vec<_>>()
            .join("\n\n");
        if let Some(owner) = &adopted_from {
            envelope = format!(
                "{envelope}\n\nThis workspace was adopted from {owner}, a finished employee — its \
                 uncommitted work is still in the checkout. Continue it; do not reset or clean \
                 anything you did not make."
            );
        }
        let prompt = AgentTaskPrompt::Assignment(envelope).resolve(&workspace, &project_path);
        let turn_id = Uuid::new_v4();
        let message_id = Uuid::new_v4();
        {
            let mut state = self.task_state.lock();
            let session = state
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("employee session is missing"))?;
            self.task_store.hydrate(session)?;
            session.project_id = project_id;
            session.provider = ticket.provider;
            session.model = Some(ticket.model.clone());
            session.reasoning_effort = ticket.reasoning_effort.clone();
            session.workspace = workspace;
            session.adopt_submitted_prompt(
                &prompt,
                turn_id,
                message_id,
                Some(employee.supervisor_id),
                false,
                None,
            );
            session.updated_at = crate::model::unix_time();
            state.mark_session_dirty(session_id);
            self.task_store.save(&mut state)?;
        }
        self.launch_prepared_session(
            session_id,
            turn_id,
            message_id,
            prompt,
            Some(employee.supervisor_id),
            &events,
        )?;
        Ok(false)
    }

    /// Resolve a `workspace: "adopt"` target. The path must be a
    /// registered worktree of `project`'s repository and owned by a daemon
    /// session whose employee ticket is finished — finished, failed,
    /// cancelled, and retired records all read `expired` here. A live
    /// owner rejects naming the employee holding it. `adopter`, when set,
    /// completes the hand-off: the old session stops claiming the path so
    /// a second adoption cannot share the checkout, and its own revival
    /// is refused rather than resumed into somebody else's worktree.
    pub(super) fn resolve_worktree_adoption(
        &self,
        project: &Path,
        adopt: &Path,
        adopter: Option<Uuid>,
    ) -> anyhow::Result<WorktreeAdoption> {
        use waku_protocol::boss::EmployeeLifecycle;
        let project = dunce::canonicalize(project)
            .with_context(|| format!("project path {} does not exist", project.display()))?;
        let adopt = dunce::canonicalize(adopt)
            .with_context(|| format!("worktree path {} does not exist", adopt.display()))?;
        if !crate::worktree::is_worktree_of(&project, &adopt) {
            bail!(
                "{} is not a registered worktree of {}",
                adopt.display(),
                project.display()
            );
        }
        let claims = |session: &AgentSession| {
            matches!(
                &session.workspace,
                SessionWorkspace::Worktree { path, adopted_by, .. }
                    if adopted_by.is_none()
                        && dunce::canonicalize(path).is_ok_and(|path| path == adopt)
            )
        };
        let mut state = self.task_state.lock();
        // The hand-off's first write is the `adopted_by` mark, its second
        // the adopter's own binding — a dispatch retried between them finds
        // the mark already crediting this adopter and finishes the move.
        let index = state.sessions.iter().position(&claims).or_else(|| {
            adopter.and_then(|adopter| {
                state.sessions.iter().position(|session| {
                    matches!(
                        &session.workspace,
                        SessionWorkspace::Worktree { path, adopted_by, .. }
                            if *adopted_by == Some(adopter)
                                && dunce::canonicalize(path).is_ok_and(|path| path == adopt)
                    )
                })
            })
        });
        let Some(index) = index else {
            if state.sessions.iter().any(|session| {
                matches!(
                    &session.workspace,
                    SessionWorkspace::Worktree { path, .. }
                        if dunce::canonicalize(path).is_ok_and(|path| path == adopt)
                )
            }) {
                bail!(
                    "{} was already adopted by another employee",
                    adopt.display()
                );
            }
            bail!(
                "{} is not a daemon-managed employee worktree",
                adopt.display()
            );
        };
        let owner_id = state.sessions[index].id;
        let mut workspace = state.sessions[index].workspace.clone();
        if let SessionWorkspace::Worktree { adopted_by, .. } = &mut workspace {
            *adopted_by = None;
        }
        let employee = self
            .boss
            .employee_including_retired(owner_id)
            .ok_or_else(|| anyhow!("{} is not owned by a summon ticket", adopt.display()))?;
        if employee.lifecycle() != EmployeeLifecycle::Expired {
            bail!(
                "{} is still owned by employee {} — its ticket is {}",
                adopt.display(),
                employee.identity.name,
                format!("{:?}", employee.lifecycle()).to_lowercase()
            );
        }
        if let Some(adopter) = adopter
            && let SessionWorkspace::Worktree { adopted_by, .. } =
                &mut state.sessions[index].workspace
            && adopted_by.is_none()
        {
            *adopted_by = Some(adopter);
            state.sessions[index].updated_at = crate::model::unix_time();
            state.mark_session_dirty(owner_id);
            self.task_store.save(&mut state)?;
        }
        Ok(WorktreeAdoption {
            owner_name: employee.identity.name,
            workspace,
        })
    }

    /// Deliver durable dispatch notifications to their supervisors.
    /// `delivered` means durably parked in the supervisor's prompt queue —
    /// a restart re-drives undelivered entries and both the session's
    /// parked mirror and its delivered messages dedupe on the derived
    /// message id, so a lost response can never duplicate the notice.
    pub(super) fn deliver_dispatch_notifications(&self) {
        let pending = self.boss.outbox_pending();
        if pending.is_empty() {
            return;
        }
        let events = self.event_source.lock().clone();
        for note in pending {
            let Some(employee) = self.boss.employee(note.session_id) else {
                let _ = self.boss.outbox_mark_delivered(note.id);
                continue;
            };
            let Some(target) = self.boss.report_target(&employee) else {
                continue;
            };
            let queued_id = Uuid::from_u128(0xD15A7C4D_u128 << 96 | u128::from(note.id));
            {
                let mut state = self.task_state.lock();
                let already = state
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == target)
                    .and_then(|session| {
                        self.task_store.hydrate(session).ok()?;
                        let parked = session
                            .queued_messages
                            .iter()
                            .any(|queued| queued.id == queued_id);
                        let delivered = session
                            .messages
                            .iter()
                            .any(|message| message.id == queued_id);
                        Some(parked || delivered)
                    })
                    .unwrap_or(false);
                if already {
                    let _ = self.boss.outbox_mark_delivered(note.id);
                    continue;
                }
            }
            let prompt = format!(
                "Employee {} ({}) has started working on {} / {}.",
                employee.identity.name,
                note.session_id,
                note.provider.display_name(),
                note.model
            );
            if self
                .queue_agent_prompt_with_id(
                    target,
                    prompt,
                    Some(note.session_id),
                    true,
                    Some(queued_id),
                    None,
                    &events,
                )
                .is_ok()
            {
                let _ = self.boss.outbox_mark_delivered(note.id);
            }
        }
    }

    /// Deliver durable wave-resolution notices to their supervisors — the
    /// same contract the dispatch outbox carries: parked-or-delivered
    /// dedupes on the derived message id, so a restart re-drives
    /// undelivered entries without ever repeating one.
    pub(super) fn deliver_wave_notifications(&self) {
        let pending = self.boss.wave_outbox_pending();
        if pending.is_empty() {
            return;
        }
        let events = self.event_source.lock().clone();
        for note in pending {
            let Some(target) = self.boss.report_target_for(note.supervisor_id) else {
                continue;
            };
            let queued_id = Uuid::from_u128(0xFEED_FACE_u128 << 96 | u128::from(note.id));
            {
                let mut state = self.task_state.lock();
                let already = state
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == target)
                    .and_then(|session| {
                        self.task_store.hydrate(session).ok()?;
                        let parked = session
                            .queued_messages
                            .iter()
                            .any(|queued| queued.id == queued_id);
                        let delivered = session
                            .messages
                            .iter()
                            .any(|message| message.id == queued_id);
                        Some(parked || delivered)
                    })
                    .unwrap_or(false);
                if already {
                    let _ = self.boss.wave_outbox_mark_delivered(note.id);
                    continue;
                }
            }
            let total = note.finished + note.failed + note.cancelled;
            let prompt = if note.cancelled == total {
                format!(
                    "Wave \"{}\" resolved — all {total} members were cancelled.",
                    note.wave_id
                )
            } else {
                let mut parts = vec![
                    format!("{} finished", note.finished),
                    format!("{} blocked/failed", note.failed),
                ];
                if note.cancelled > 0 {
                    parts.push(format!("{} cancelled", note.cancelled));
                }
                format!("Wave \"{}\" resolved — {}.", note.wave_id, parts.join(", "))
            };
            if self
                .queue_agent_prompt_with_id(
                    target,
                    prompt,
                    None,
                    true,
                    Some(queued_id),
                    None,
                    &events,
                )
                .is_ok()
            {
                let _ = self.boss.wave_outbox_mark_delivered(note.id);
            }
        }
    }

    /// Restart reconciliation for one interrupted employee. Queued records
    /// never reach here. `dispatching` consults the persisted turn
    /// identity: a session whose first prompt already adopted finishes as
    /// interrupted like any working employee, while an unstarted launch
    /// reverts to the queue — replaying it would double-dispatch, and
    /// reporting it would mourn a job that never ran.
    pub(crate) fn recover_boss_employee(&self, session_id: Uuid) -> anyhow::Result<()> {
        use waku_protocol::boss::EmployeeLifecycle;
        let Some(entry) = self.boss.employee(session_id) else {
            return Ok(());
        };
        match entry.lifecycle() {
            EmployeeLifecycle::Dispatching => {
                let started = self
                    .task_state
                    .lock()
                    .sessions
                    .iter()
                    .find(|session| session.id == entry.session_id)
                    .is_some_and(|session| session.has_started());
                if started {
                    self.finish_boss_employee(
                        entry.session_id,
                        false,
                        waku_protocol::boss::EmployeeSettle::Restarted,
                    )
                } else {
                    self.boss.revert_dispatch(entry.session_id)?;
                    self.wake_summon_queue();
                    Ok(())
                }
            }
            EmployeeLifecycle::Finishing => self.finish_boss_employee_tail(
                session_id,
                &entry,
                waku_protocol::boss::EmployeeSettle::Restarted,
            ),
            _ => self.finish_boss_employee(
                entry.session_id,
                false,
                waku_protocol::boss::EmployeeSettle::Restarted,
            ),
        }
    }

    /// `summon`: validate and resolve everything before accepting —
    /// caller, persona, grants, prompt, project, resource shape, and a
    /// concrete provider+model — then persist the one admission ticket:
    /// employee record, task shell, and queue sequence. Capacity is a
    /// wait reason, never an RPC error: the answer is `queued` (or the
    /// state the ticket reached before the bounded wait elapsed), and
    /// the scheduler claims a slot and launches asynchronously.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn summon_employee(
        &self,
        caller: Option<Uuid>,
        persona_id: Uuid,
        job_title: String,
        prompt: String,
        project: String,
        provider: Option<ProviderKind>,
        model: Option<String>,
        reasoning_effort: Option<String>,
        workspace: Option<AgentWorkspace>,
        base_branch: Option<String>,
        adopt_worktree: Option<PathBuf>,
        permissions: Option<waku_protocol::boss::PermissionOverrides>,
        work_goal: waku_protocol::boss::EmployeeGoal,
        icon: Option<waku_protocol::custom_commands::CustomCommandIcon>,
        resources: Option<waku_protocol::resources::ResourceSet>,
        allow_burst: bool,
        group_id: Option<String>,
        priority: Option<i64>,
        goal_id: Option<Uuid>,
        plan: Option<String>,
        item: Option<Uuid>,
        request_id: Option<Uuid>,
        events: &EventSink,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        let supervisor = caller
            .or(self.boss.document().session_id)
            .ok_or_else(|| anyhow!("open the boss before summoning employees"))?;
        if prompt.trim().is_empty() {
            bail!("agent sessions require a prompt");
        }
        // Adoption validates at admission — the summoner hears a bad path
        // or a live owner immediately — and again at dispatch, since a
        // queued ticket's target can be claimed or revived meanwhile.
        let adopt_worktree = match (workspace, adopt_worktree) {
            (Some(AgentWorkspace::Adopt), Some(path)) => Some(path),
            (Some(AgentWorkspace::Adopt), None) => {
                bail!("adopt workspaces require an adoptWorktree path")
            }
            (_, Some(_)) => bail!("adoptWorktree only applies to workspace \"adopt\""),
            _ => None,
        };
        let fingerprint = serde_json::to_string(&serde_json::json!({
            "personaId": persona_id, "jobTitle": job_title, "prompt": prompt,
            "project": project, "provider": provider, "model": model,
            "reasoningEffort": reasoning_effort, "workspace": workspace,
            "baseBranch": base_branch, "adoptWorktree": adopt_worktree,
            "permissions": permissions,
            "workGoal": work_goal, "icon": icon, "resources": resources,
            "allowBurst": allow_burst, "groupId": group_id,
            "priority": priority, "goalId": goal_id,
            "plan": plan, "item": item,
        }))?;
        if let Some(request_id) = request_id
            && let Some(existing) = self
                .boss
                .document()
                .employees
                .iter()
                .find(|entry| entry.request_id == Some(request_id))
                .cloned()
        {
            if existing.request_fingerprint.as_deref() == Some(fingerprint.as_str()) {
                // Lost-response retry — the original admission stands.
                return self.summoned_result(existing.session_id);
            }
            bail!("requestId was already used for a different summon");
        }
        // The plan tag resolves at admission — unknown or closed plans and
        // unknown or finished items fail here rather than landing untagged.
        let plan_tag = match (&plan, item) {
            (Some(reference), item) => Some(self.boss.plan_assignment(reference, item)?),
            (None, Some(_)) => bail!("a summon item tag requires a plan"),
            (None, None) => None,
        };
        if let Some(adopt) = &adopt_worktree {
            self.resolve_worktree_adoption(Path::new(&project), adopt, None)?;
        }
        let mut employee = self.boss.prepare_employee(
            supervisor,
            persona_id,
            job_title,
            permissions,
            work_goal,
            icon,
        )?;
        let selection = AgentCreateSelection {
            provider,
            model,
            title: Some(employee.identity.name.clone()),
            reasoning_effort,
            service_tier: None,
            context_window: None,
        };
        let resolved = self.resolve_agent_task_selection(
            Some(supervisor),
            &selection,
            Path::new(&project),
            &prompt,
            true,
        )?;
        // A ticket counts a concrete provider+model pair — never an
        // ambiguous `default` bucket. When neither the request nor the
        // catalog names one, the summoner must say so explicitly.
        let model_id = resolved
            .model
            .clone()
            .or_else(|| resolved.concrete_model.clone())
            .ok_or_else(|| {
                anyhow!("could not resolve a concrete model — pass `model` explicitly")
            })?;
        let resources = resources.unwrap_or_default();
        if !resource_set_empty(&resources) {
            self.resource_broker()?.validate_set(&resources)?;
        }
        let ticket = waku_protocol::boss::SummonTicket {
            sequence: 0,
            generation: 1,
            provider: resolved.provider,
            model: model_id,
            reasoning_effort: resolved.reasoning_effort.clone(),
            prompt: prompt.clone(),
            project: project.clone(),
            workspace,
            base_branch,
            adopt_worktree,
            resources,
            allow_burst,
            pending_prompts: Vec::new(),
            group_id,
            priority,
            goal_id,
            reservation: None,
            pending_resources: None,
            pending_reservation: None,
            blocked_by: Vec::new(),
            dispatch_event: None,
            interruptions: Vec::new(),
            resume_count: 0,
            last_resumed_cause: None,
        };
        employee.request_id = request_id;
        employee.request_fingerprint = request_id.map(|_| fingerprint);
        if let Some((plan_id, item_id)) = plan_tag {
            employee.plan_id = Some(plan_id);
            employee.item_id = item_id;
        }
        let employee_name = employee.identity.name.clone();
        let employee_title = employee.job_title.clone();
        // The summon marker freezes the identity the card shows — name,
        // avatar seed, job title, and the icon the roster would resolve —
        // so the transcript keeps rendering it after the roster record
        // retires or the employee's task archives.
        let summon_card = {
            let document = self.boss.document();
            waku_protocol::model::BossSummonCard {
                session_id: employee.session_id,
                name: employee_name.clone(),
                avatar_seed: employee.identity.avatar_seed.clone(),
                job_title: employee_title.clone(),
                icon: employee
                    .icon
                    .filter(|icon| icon.is_employee_icon())
                    .or_else(|| {
                        document
                            .personas
                            .iter()
                            .find(|persona| persona.id == employee.persona_id)
                            .and_then(|persona| persona.icon)
                            .filter(|icon| icon.is_employee_icon())
                    }),
            }
        };
        // The task shell lands before the roster does — a client that
        // opens the new employee immediately finds the assignment's
        // session, still unstarted: no worktree, no runtime, no claims.
        self.create_employee_shell(&employee, &resolved, &ticket)?;
        let session_id = employee.session_id;
        if let Err(error) = self.boss.enqueue_ticket(employee, ticket) {
            self.remove_session_shell(session_id);
            return Err(error);
        }
        // The summon marker lands in the supervisor's transcript now —
        // the card reads the roster for live status and shows its queued
        // state until the ticket dispatches.
        let mut marker = crate::model::ActivityItem::new(
            None,
            crate::model::ActivityKind::Tool,
            format!("Summoned {employee_name} — {employee_title}"),
            None,
            true,
        )
        .with_tool_name(Some(waku_protocol::model::BOSS_SUMMON_TOOL_NAME));
        marker.arguments = Some(serde_json::to_string(&summon_card)?);
        let event = DriverEvent::RichActivity(marker);
        record_boss_event(&self.task_state, &self.task_store, supervisor, &event)?;
        let _ = events.send(event_to_wire(event)?);
        self.wake_summon_queue();
        // Answer with the state the ticket actually reached — a free slot
        // dispatches nearly synchronously, so most summons still return
        // `working`/`dispatching` rather than `queued`.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match self.boss.employee_lifecycle(session_id) {
                Some(state) if state != waku_protocol::boss::EmployeeLifecycle::Queued => break,
                _ if std::time::Instant::now() >= deadline => break,
                _ => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
        let result = self.summoned_result(session_id)?;
        // A launch that failed inside the wait surfaces as the summon
        // error it used to be — the ticket is expired either way, and a
        // supervisor that scripted on failure sees it immediately rather
        // than in the finish report.
        if let waku_protocol::boss::BossResult::Summoned {
            state: waku_protocol::boss::EmployeeLifecycle::Expired,
            ..
        } = result
            && let Some(error) = self
                .boss
                .employee(session_id)
                .and_then(|employee| employee.blocker)
                .filter(|note| note.starts_with("Employee launch failed"))
        {
            bail!("{error}");
        }
        Ok(result)
    }

    /// The `Summoned` payload for the employee's current state —
    /// `queued` carries position and wait reasons; dispatched states
    /// carry the resolved selection.
    pub(super) fn summoned_result(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        use waku_protocol::boss::{BossResult, SummonAdmission};
        let employee = self
            .boss
            .employee(session_id)
            .ok_or_else(|| anyhow!("employee record is missing"))?;
        Ok(BossResult::Summoned {
            session_id,
            state: employee.lifecycle(),
            admission: employee.ticket.as_ref().map(|ticket| SummonAdmission {
                provider: ticket.provider,
                model: ticket.model.clone(),
                reasoning_effort: ticket.reasoning_effort.clone(),
                queue_position: self.boss.queue_position(session_id),
                blocked_by: ticket.blocked_by.clone(),
            }),
        })
    }

    /// The minimal managed task a queued ticket owns: registered project,
    /// stamped session, resolved selection — but no adopted prompt,
    /// worktree, runtime, or claims until dispatch.
    pub(super) fn create_employee_shell(
        &self,
        employee: &waku_protocol::boss::BossEmployee,
        resolved: &ResolvedAgentSelection,
        ticket: &waku_protocol::boss::SummonTicket,
    ) -> anyhow::Result<()> {
        let project = dunce::canonicalize(Path::new(&ticket.project))
            .with_context(|| format!("project path {} does not exist", ticket.project))?;
        let (project_id, _) = self.register_agent_project(&project)?;
        let mut session = AgentSession::new(project_id, resolved.provider);
        session.id = employee.session_id;
        session.set_title(&employee.identity.name);
        session.agent_rename_allowed = false;
        session.boss_managed = true;
        session.runtime_mode = resolved.sender_mode;
        session.environment = resolved.sender_environment;
        session.model = Some(ticket.model.clone());
        if let Some(run) = &resolved.routed {
            session.route_decision = Some(run.decision.clone());
        }
        session.reasoning_effort = resolved.reasoning_effort.clone();
        session.service_tier = resolved.service_tier.clone();
        session.context_window = resolved.context_window.clone();
        let mut state = self.task_state.lock();
        state.push_session(session);
        self.task_store.save(&mut state)?;
        Ok(())
    }

    /// Drop a task shell whose admission never committed — best-effort
    /// cleanup so an orphan shell cannot surface as a real employee task.
    pub(super) fn remove_session_shell(&self, session_id: Uuid) {
        let mut state = self.task_state.lock();
        let before = state.sessions.len();
        state.sessions.retain(|session| session.id != session_id);
        if state.sessions.len() != before {
            let _ = self.task_store.save(&mut state);
        }
    }

    /// Controls a queued employee's ticket. `prompt` appends durable
    /// instructions to the dispatch envelope; `steer` cannot steer a turn
    /// that does not exist; `stop` cancels the pending work; model and
    /// workspace edits rewrite the ticket in place, keeping its sequence.
    pub(super) fn control_queued_employee(
        &self,
        caller: Option<Uuid>,
        session_id: Uuid,
        action: waku_protocol::boss::EmployeeControl,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        use waku_protocol::boss::{BossResult, EmployeeControl};
        match action {
            EmployeeControl::Prompt { prompt, delivery } => {
                if prompt.trim().is_empty() {
                    bail!("employee prompts cannot be empty");
                }
                if matches!(delivery.unwrap_or_default(), AgentPromptDelivery::Steer) {
                    bail!("employee is queued, not running — steer needs a live turn");
                }
                if !self.boss.append_queued_prompt(session_id, prompt)? {
                    bail!("employee is no longer queued");
                }
                Ok(BossResult::Saved)
            }
            EmployeeControl::Steer { .. } => {
                bail!("employee is queued, not running — steer needs a live turn")
            }
            EmployeeControl::Stop => {
                record_boss_event(
                    &self.task_state,
                    &self.task_store,
                    session_id,
                    &DriverEvent::TurnFinished {
                        success: false,
                        summary: Some("Cancelled while queued".into()),
                        summary_i18n: None,
                    },
                )?;
                self.boss.mark_cancelled(session_id)?;
                self.finish_boss_employee(
                    session_id,
                    false,
                    waku_protocol::boss::EmployeeSettle::Stopped,
                )?;
                Ok(BossResult::Saved)
            }
            EmployeeControl::SetModel {
                provider,
                model,
                reasoning_effort,
                ..
            } => {
                let effort = self.validate_employee_model(provider, &model, reasoning_effort)?;
                if !self.boss.reticket(session_id, |ticket| {
                    ticket.provider = provider;
                    ticket.model = model.clone();
                    ticket.reasoning_effort = effort;
                })? {
                    bail!("employee is no longer queued");
                }
                self.wake_summon_queue();
                Ok(BossResult::Saved)
            }
            EmployeeControl::SetResources { resources } => {
                if !resource_set_empty(&resources) {
                    self.resource_broker()?.validate_set(&resources)?;
                }
                if !self.boss.reticket(session_id, |ticket| {
                    ticket.resources = resources.clone();
                })? {
                    bail!("employee is no longer queued");
                }
                // The next scheduler pass re-attempts admission against the
                // edited set — a ticket that now over-declares simply waits.
                self.wake_summon_queue();
                Ok(BossResult::Saved)
            }
            EmployeeControl::SetWorkspace {
                workspace,
                base_branch,
            } => {
                if matches!(workspace, AgentWorkspace::Adopt) {
                    bail!(
                        "adopting a worktree is summon-only — setWorkspace accepts \"local\" or \"worktree\""
                    );
                }
                if matches!(workspace, AgentWorkspace::Worktree)
                    && base_branch
                        .as_deref()
                        .is_none_or(|branch| branch.trim().is_empty())
                {
                    bail!("worktree workspaces require a base branch");
                }
                if !self.boss.reticket(session_id, |ticket| {
                    ticket.workspace = Some(workspace);
                    ticket.base_branch = base_branch.clone();
                    ticket.adopt_worktree = None;
                })? {
                    bail!("employee is no longer queued");
                }
                Ok(BossResult::Saved)
            }
            EmployeeControl::SetPermissions { permissions } => {
                self.boss
                    .set_employee_permissions(caller, session_id, permissions)?;
                self.boss.reset_context(session_id);
                Ok(BossResult::Saved)
            }
            EmployeeControl::SetPlan { plan, item } => {
                self.boss.set_employee_plan(session_id, plan, item)?;
                Ok(BossResult::Saved)
            }
        }
    }

    /// `setResources` on a live employee: park the new set on its ticket
    /// under a fresh reservation id and let the scheduler re-admit it in
    /// place. The running turn is never interrupted — a set that cannot
    /// grant yet simply waits on the record while the employee keeps its
    /// current claims.
    pub(super) fn control_employee_resources(
        &self,
        session_id: Uuid,
        resources: waku_protocol::resources::ResourceSet,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        self.boss.require_active(session_id)?;
        if !resource_set_empty(&resources) {
            self.resource_broker()?.validate_set(&resources)?;
        }
        let (base, _started) = self.ticket_base_for(session_id)?;
        let reservation = Uuid::new_v4();
        let replaced =
            self.boss
                .request_resource_update(session_id, base, resources, reservation)?;
        // A superseded parked id may already hold a grant — release it so
        // the ledger never orphans capacity.
        if let Some(stale) = replaced
            && let Ok(broker) = self.resource_broker()
        {
            broker.release_admission(session_id, stale);
        }
        self.wake_summon_queue();
        Ok(waku_protocol::boss::BossResult::Saved)
    }

    /// Re-enter an employee into admission — resurrection and `setModel`
    /// share it. The durable transition lands first (queued, new
    /// generation, tail sequence); the caller then tears down the old
    /// runtime and releases the previous generation's claims, so a crash
    /// leaves a queued ticket rather than a silently dead slot. Prompts
    /// parked for the old lifetime move onto the ticket's pending list —
    /// ahead of whatever this requeue adds — and their mirrored chips
    /// leave the document so dispatch does not deliver them twice.
    pub(super) fn requeue_employee(
        &self,
        session_id: Uuid,
        adjust: impl FnOnce(&mut waku_protocol::boss::SummonTicket, bool),
    ) -> anyhow::Result<()> {
        let (base, started) = self.ticket_base_for(session_id)?;
        let mut parked = Vec::new();
        {
            let mut state = self.task_state.lock();
            if let Some(session) = state
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            {
                self.task_store.hydrate(session)?;
                if session
                    .queued_messages
                    .iter()
                    .any(|queued| queued.is_agent_owned())
                {
                    parked = session
                        .queued_messages
                        .iter()
                        .filter(|queued| queued.is_agent_owned())
                        .map(|queued| queued.content.clone())
                        .collect();
                    session
                        .queued_messages
                        .retain(|queued| !queued.is_agent_owned());
                    session.updated_at = crate::model::unix_time();
                    state.mark_session_dirty(session_id);
                    self.task_store.save(&mut state)?;
                }
            }
        }
        self.agent.clear_session(session_id);
        let (_employee, stale_reservations) =
            self.boss.requeue_employee(session_id, base, |ticket| {
                if !parked.is_empty() {
                    let mut combined = parked;
                    combined.extend(std::mem::take(&mut ticket.pending_prompts));
                    ticket.pending_prompts = combined;
                }
                adjust(ticket, started);
            })?;
        for reservation in stale_reservations {
            if let Ok(broker) = self.resource_broker() {
                broker.release_admission(session_id, reservation);
            }
        }
        self.wake_summon_queue();
        Ok(())
    }

    /// `resume`: re-admit an interrupted employee in place. The resume is
    /// the ordinary requeue — same capacity admission, same parked-prompt
    /// merge, same adopted-worktree refusal — with the ticket adjusting
    /// to carry a synthesized "verify and continue" prompt behind
    /// whatever parked at expiry, plus the resume count and the cause it
    /// answered. A live record refuses: prompts reach it directly.
    pub(super) fn resume_employee(&self, session_id: Uuid) -> anyhow::Result<()> {
        let employee = self
            .boss
            .employee_including_retired(session_id)
            .ok_or_else(|| anyhow!("{session_id} is not a Boss employee"))?;
        if !employee.expired {
            bail!("employee {session_id} is still live — prompt it instead of resuming");
        }
        let expiry = employee.expiry.as_ref();
        let cause = expiry
            .map(|expiry| expiry.cause)
            .unwrap_or(waku_protocol::boss::ExpiryCause::Finished);
        let mut prompt = format!(
            "You were interrupted — {}. Verify the state of your partial work before continuing where it left off.",
            cause.describe()
        );
        if let Some(question) = expiry.and_then(|expiry| expiry.pending_question.as_deref()) {
            prompt.push_str(&format!(" Your unanswered question was: \"{question}\""));
        }
        self.requeue_employee(session_id, |ticket, _started| {
            ticket.pending_prompts.push(prompt.clone());
            ticket.resume_count = ticket.resume_count.saturating_add(1);
            ticket.last_resumed_cause = Some(cause);
        })
    }

    /// A synthesized ticket for employees whose records predate admission
    /// tickets — a pre-queue summon requeuing for a prompt or a model
    /// change. `requeue_employee` keeps a real ticket when one exists;
    /// this fills the gap with the session's resolved state. The bool is
    /// whether the session ever started — it decides whether requeued
    /// prompts fold into the original envelope or replay as new turns.
    pub(super) fn ticket_base_for(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<(waku_protocol::boss::SummonTicket, bool)> {
        let (provider, model, effort, project, workspace, base_branch, started) = {
            let mut state = self.task_state.lock();
            let index = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("employee session is missing"))?;
            self.task_store.hydrate(&mut state.sessions[index])?;
            let session = &state.sessions[index];
            let project = state
                .projects
                .iter()
                .find(|project| project.id == session.project_id)
                .map(|project| project.path.display().to_string())
                .unwrap_or_default();
            // A worktree a later summon adopted is no longer this
            // employee's to resume — revival would land it in somebody
            // else's checkout, so the requeue refuses and the transcript
            // stays read-only history.
            if let SessionWorkspace::Worktree {
                adopted_by: Some(adopter),
                ..
            } = &session.workspace
            {
                let adopter = state
                    .sessions
                    .iter()
                    .find(|session| &session.id == adopter)
                    .and_then(|session| self.boss.employee(session.id))
                    .map(|employee| employee.identity.name)
                    .unwrap_or_else(|| "another employee".to_owned());
                bail!(
                    "the employee's worktree was adopted by {adopter}; summon a fresh employee instead of resuming it"
                );
            }
            let (workspace, base_branch) = match &session.workspace {
                SessionWorkspace::Worktree { base_branch, .. } => {
                    (AgentWorkspace::Worktree, base_branch.clone())
                }
                _ => (AgentWorkspace::Local, None),
            };
            (
                session.provider,
                session.model.clone(),
                session.reasoning_effort.clone(),
                project,
                workspace,
                base_branch,
                session.has_started(),
            )
        };
        let model = model
            .or_else(|| {
                let catalog = crate::model_catalog::cached_models(provider)
                    .unwrap_or_else(|| crate::model_catalog::fallback_models(provider));
                catalog
                    .iter()
                    .find(|entry| entry.is_default)
                    .or_else(|| catalog.first())
                    .map(|entry| entry.id.clone())
            })
            .ok_or_else(|| anyhow!("could not resolve a concrete model for re-admission"))?;
        Ok((
            waku_protocol::boss::SummonTicket {
                sequence: 0,
                generation: 0,
                provider,
                model,
                reasoning_effort: effort,
                prompt: String::new(),
                project,
                workspace: Some(workspace),
                base_branch,
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
                blocked_by: Vec::new(),
                dispatch_event: None,
                interruptions: Vec::new(),
                resume_count: 0,
                last_resumed_cause: None,
            },
            started,
        ))
    }

    /// Catalog validation shared by `setModel` on queued and working
    /// employees — the provider's catalog must list the model and any
    /// pinned effort. Returns the normalized effort pin.
    pub(super) fn validate_employee_model(
        &self,
        provider: ProviderKind,
        model: &str,
        reasoning_effort: Option<String>,
    ) -> anyhow::Result<Option<String>> {
        let catalog = crate::model_catalog::cached_models(provider)
            .unwrap_or_else(|| crate::model_catalog::fallback_models(provider));
        let selected =
            waku_protocol::model_catalog::packed_catalog_model(&catalog, model, provider)
                .ok_or_else(|| {
                    anyhow!(
                        "model {model:?} is not listed for {}",
                        provider.display_name()
                    )
                })?;
        reasoning_effort
            .map(|effort| {
                if effort == "default" {
                    Ok(None)
                } else if selected
                    .model
                    .reasoning_efforts
                    .iter()
                    .any(|option| option.id == effort)
                {
                    Ok(Some(effort))
                } else {
                    Err(anyhow!(
                        "reasoning effort is not supported by model {model:?}"
                    ))
                }
            })
            .transpose()
            .map(|effort| effort.flatten())
    }

    /// Interrupt delivery for an employee's report: steer into the
    /// supervisor's open turn when its runtime can take one, else park a
    /// hidden prompt that drains when the turn settles.
    pub(super) fn deliver_employee_report(
        &self,
        target: Uuid,
        prompt: String,
        sender: Uuid,
        report_trigger: Option<crate::model::ReportTrigger>,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        let report_trigger = report_trigger.map(|mut trigger| {
            let state = self.task_state.lock();
            trigger.reference_context = state
                .sessions
                .iter()
                .find(|session| session.id == sender)
                .and_then(|session| {
                    state
                        .projects
                        .iter()
                        .find(|project| project.id == session.project_id)
                        .map(|project| crate::model::ReferenceContext {
                            project_root: project.path.clone(),
                            worktree: session.workspace.path().map(Path::to_path_buf),
                        })
                });
            trigger
        });
        let driver = self
            .sessions
            .lock()
            .get(&target)
            .map(|entry| entry.driver.clone());
        if let Some(driver) = driver
            && self.agent.has_open_turn(target)
            && driver.supports_steer()
        {
            let transport = agent_prompt_envelope(&self.task_state, target, Some(sender), &prompt);
            self.agent.record_pending_steer(
                target,
                crate::agent::AgentPrompt {
                    prompt: prompt.clone(),
                    transport: transport.clone(),
                    sender: Some(sender),
                    // A direct steer never parks — no chip to mirror.
                    queued_id: None,
                    context: None,
                    hidden: true,
                    report_trigger: report_trigger.map(|mut trigger| {
                        trigger.boundary = crate::model::ReportTriggerBoundary::Steer;
                        trigger
                    }),
                },
            );
            driver.steer(transport.unwrap_or(prompt));
            return Ok(());
        }
        self.queue_agent_prompt_with_id(
            target,
            prompt,
            Some(sender),
            true,
            None,
            report_trigger,
            events,
        )
    }
}
