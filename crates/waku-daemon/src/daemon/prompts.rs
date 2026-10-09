use super::*;

impl WakuBackend {
    /// The session's driver when it can take a steer into an open turn —
    /// interrupt delivery and the strict steer op share this gate.
    pub(super) fn steerable_driver(&self, session_id: Uuid) -> Option<DriverHandle> {
        let driver = self
            .sessions
            .lock()
            .get(&session_id)
            .map(|entry| entry.driver.clone())?;
        (driver.supports_steer() && self.agent.has_open_turn(session_id)).then_some(driver)
    }

    /// Record and send a steer into `driver`'s open turn — callers gate
    /// on `steerable_driver` (or their own error wording) first.
    pub(super) fn send_agent_steer(
        &self,
        driver: &DriverHandle,
        session_id: Uuid,
        prompt: String,
        sender: Option<Uuid>,
    ) {
        let transport = agent_prompt_envelope(&self.task_state, session_id, sender, &prompt);
        self.agent.record_pending_steer(
            session_id,
            crate::agent::AgentPrompt {
                prompt: prompt.clone(),
                transport: transport.clone(),
                sender,
                // A direct steer never parks — no chip to mirror.
                queued_id: None,
                context: None,
                hidden: false,
                report_trigger: None,
            },
        );
        driver.steer(transport.unwrap_or(prompt));
        // A steer landing mid-turn is the supervisor's answer to a raised
        // blocker — the flagged presentation goes stale the moment fresh
        // direction arrives, not when the employee next expires.
        if let Err(error) = self.boss.clear_employee_blocker(session_id) {
            eprintln!("could not clear a Boss employee's blocker: {error:#}");
        }
    }

    /// `agent prompt`: deliver a message to an existing task, by Waku task
    /// id or provider-native thread id. Queue mode holds the prompt in a
    /// daemon-side per-session queue until the target is idle; steer mode
    /// injects it into the running turn; interrupt mode steers when a turn
    /// is open and queues otherwise.
    pub(super) fn agent_prompt(
        &self,
        sender: Option<Uuid>,
        task_id: Option<Uuid>,
        thread_id: Option<String>,
        provider: Option<ProviderKind>,
        prompt: String,
        delivery: AgentPromptDelivery,
        events: EventSink,
    ) -> anyhow::Result<ResponsePayload> {
        self.require_agent_tools()?;
        if prompt.trim().is_empty() {
            bail!("agent prompts require a prompt");
        }
        let sender_employee = sender.and_then(|sender| self.boss.employee(sender));
        let target = if let Some(employee) = &sender_employee {
            self.boss.require_active(employee.session_id)?;
            let supervisor = self
                .boss
                .report_target(employee)
                .ok_or_else(|| anyhow!("your supervisor is unavailable"))?;
            // Validate explicit addresses too: even an unknown task UUID must
            // explain the employee's channel rather than silently misdeliver.
            let addressed = if thread_id.is_some() {
                self.resolve_agent_target(task_id, thread_id, provider)?
            } else {
                task_id.unwrap_or(supervisor)
            };
            if addressed != supervisor {
                bail!(
                    "employees report upward only — use `goddard-agent steer-supervisor --text TEXT`, \
                       `goddard-agent boss report-blocker` when you cannot proceed, or \
                       `goddard-agent merge submit` to land work"
                );
            }
            supervisor
        } else {
            if task_id.is_none() && thread_id.is_none() {
                bail!(
                    "steer-supervisor is employee-only; use `goddard-agent prompt TASK_ID --text TEXT`"
                );
            }
            self.resolve_agent_target(task_id, thread_id, provider)?
        };
        if sender_employee.is_some() {
            // Employee messages never enter either prompt queue and never
            // wait for the sender's turn to settle. Queue/steer flags on the
            // legacy prompt command cannot change this contract.
            self.boss.require_active(target)?;
            if self.session_quarantined(target) {
                bail!("received files are quarantined until trusted");
            }
            if self
                .boss
                .employee_lifecycle(target)
                .is_some_and(|state| state != waku_protocol::boss::EmployeeLifecycle::Working)
            {
                bail!("your supervisor is not ready to receive a steer; retry when it is working");
            }
            let (runtime_id, driver) = self.ensure_agent_runtime(target, &events)?;
            if self.agent.has_open_turn(target) {
                if !driver.supports_steer() {
                    bail!(
                        "your supervisor's provider does not support steering; message was not queued"
                    );
                }
                self.send_agent_steer(&driver, target, prompt, sender);
            } else {
                deliver_agent_prompt(
                    target,
                    &driver,
                    crate::agent::AgentPrompt {
                        prompt,
                        transport: None,
                        sender,
                        queued_id: None,
                        context: None,
                        hidden: false,
                        report_trigger: None,
                    },
                    &events.for_session(target, runtime_id),
                    &self.agent,
                    &self.auto_prompts,
                    &self.task_state,
                    &self.task_store,
                    &self.boss,
                    &self.automations,
                )?;
            }
            return Ok(ResponsePayload::Ack);
        }
        if sender.is_some_and(|id| self.boss.is_managed(id)) || self.boss.is_managed(target) {
            use waku_protocol::boss::EmployeeLifecycle;
            self.boss.require_control(sender, target)?;
            // Queue delivery to a queued ticket joins its dispatch
            // envelope; a prompt to a finished employee re-enters
            // admission and resumes the same transcript once dispatched.
            match self.boss.employee_lifecycle(target) {
                Some(EmployeeLifecycle::Queued) => {
                    if !self.boss.append_queued_prompt(target, prompt.clone())? {
                        bail!("employee {target} is no longer queued");
                    }
                    return Ok(ResponsePayload::Ack);
                }
                Some(EmployeeLifecycle::Dispatching) => {
                    // Mid-launch — the parked queue drains once the
                    // runtime lands.
                    self.queue_agent_prompt(target, prompt, sender, &events)?;
                    return Ok(ResponsePayload::Ack);
                }
                Some(EmployeeLifecycle::Finishing) => {
                    bail!("employee {target} is finishing; try again in a moment")
                }
                _ => {}
            }
            let was_expired = self
                .boss
                .employee(target)
                .is_some_and(|employee| employee.expired)
                || self
                    .boss
                    .document()
                    .retired_employees
                    .iter()
                    .any(|employee| employee.session_id == target);
            if was_expired {
                self.requeue_employee(target, |ticket, started| {
                    if started {
                        ticket.prompt.clear();
                        ticket.pending_prompts.push(prompt.clone());
                    } else {
                        ticket.prompt = prompt.clone();
                        ticket.pending_prompts.clear();
                    }
                })?;
                return Ok(ResponsePayload::Ack);
            }
            self.boss.require_active(target)?;
        }
        if self.session_quarantined(target) {
            bail!("received files are quarantined until trusted");
        }
        if employee_update_streaming(target, sender, &self.agent, &self.boss) {
            self.queue_agent_prompt(target, prompt, sender, &events)?;
            return Ok(ResponsePayload::Ack);
        }
        match delivery {
            AgentPromptDelivery::Interrupt => {
                match self.steerable_driver(target) {
                    Some(driver) => self.send_agent_steer(&driver, target, prompt, sender),
                    None => self.queue_agent_prompt(target, prompt, sender, &events)?,
                }
                Ok(ResponsePayload::Ack)
            }
            AgentPromptDelivery::Steer => {
                let driver = self
                    .sessions
                    .lock()
                    .get(&target)
                    .map(|entry| entry.driver.clone())
                    .ok_or_else(|| anyhow!("task {target} has no running session to steer"))?;
                if !self.agent.has_open_turn(target) {
                    bail!("task {target} has no running turn to steer");
                }
                if !driver.supports_steer() {
                    bail!("the task's provider does not support steering");
                }
                self.send_agent_steer(&driver, target, prompt, sender);
                Ok(ResponsePayload::Ack)
            }
            AgentPromptDelivery::Queue => {
                self.queue_agent_prompt(target, prompt, sender, &events)?;
                Ok(ResponsePayload::Ack)
            }
        }
    }

    /// Queue-mode delivery shared with the automation scheduler: the prompt
    /// waits behind any open turn and drains once the session goes idle.
    /// Enqueueing before the working check means a prompt can never slip
    /// between a finishing turn and its queue drain.
    pub(crate) fn queue_agent_prompt(
        &self,
        target: Uuid,
        prompt: String,
        sender: Option<Uuid>,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        self.queue_agent_prompt_with_id(target, prompt, sender, false, None, None, events)
    }

    pub(super) fn queue_agent_prompt_hidden(
        &self,
        target: Uuid,
        prompt: String,
        sender: Option<Uuid>,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        self.queue_agent_prompt_with_id(target, prompt, sender, true, None, None, events)
    }

    /// A deterministic `queued_id` rides durable deliveries — dispatch
    /// notifications — so redelivery dedupes on it through both the parked
    /// mirror and the delivered message row.
    pub(super) fn queue_agent_prompt_with_id(
        &self,
        target: Uuid,
        prompt: String,
        sender: Option<Uuid>,
        hidden: bool,
        queued_id: Option<Uuid>,
        report_trigger: Option<crate::model::ReportTrigger>,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        let queued_id = queued_id.unwrap_or_else(Uuid::new_v4);
        // The parked id doubles as the delivered message's — the trigger's
        // dedupe identity matches it so a redriven delivery folds onto the
        // same marker.
        let report_trigger = report_trigger.map(|mut trigger| {
            trigger.event_id = queued_id;
            trigger
        });
        self.agent.enqueue(
            target,
            crate::agent::AgentPrompt {
                prompt: prompt.clone(),
                // Wrapped at delivery so a renamed sender and a late
                // relationship read their current values.
                transport: None,
                sender,
                queued_id: Some(queued_id),
                context: None,
                hidden,
                report_trigger: report_trigger.clone(),
            },
        );
        let managed = self.boss.is_managed(target);
        if managed {
            // Result indexes must survive a failed supervisor launch and a
            // subsequent daemon restart, even when the supervisor was idle.
            mirror_agent_queued_prompt(
                &self.task_state,
                &self.task_store,
                target,
                queued_id,
                &prompt,
                sender,
                hidden,
                report_trigger.clone(),
            )?;
        }
        // A queued or dispatching ticket owns no settled runtime — park
        // the prompt in the mirrored queue; dispatch drains it when the
        // launch lands.
        // A workspace move likewise must not launch or deliver into the
        // old directory; its continuation drains these prompts after the move.
        if self
            .boss
            .employee(target)
            .is_some_and(|employee| employee.workspace_transition)
        {
            return Ok(());
        }
        if self.boss.employee_lifecycle(target).is_some_and(|state| {
            matches!(
                state,
                waku_protocol::boss::EmployeeLifecycle::Queued
                    | waku_protocol::boss::EmployeeLifecycle::Dispatching
            )
        }) {
            return Ok(());
        }
        if self.agent.is_working(target)
            || employee_update_streaming(target, sender, &self.agent, &self.boss)
        {
            // The runtime event forwarder delivers queued prompts in
            // order once the provider finishes the turn. Mirror the wait
            // into the session document so every client renders the parked
            // prompt as a queued follow-up chip.
            if !managed {
                mirror_agent_queued_prompt(
                    &self.task_state,
                    &self.task_store,
                    target,
                    queued_id,
                    &prompt,
                    sender,
                    hidden,
                    report_trigger,
                )?;
            }
            if let Some(runtime_id) = self.runtime_id_for(target) {
                send_agent_queue_changed(
                    &self.task_state,
                    &events.for_session(target, runtime_id),
                    target,
                );
            }
            // Wake even if the sender settled after the hold check.
            if managed {
                self.wake_summon_queue();
            }
            return Ok(());
        }
        let (runtime_id, driver) = self.ensure_agent_runtime(target, events)?;
        let sink = events.for_session(target, runtime_id);
        self.drain_agent_queue(target, &driver, &sink)
    }

    pub(crate) fn auto_prompt_settings(&self) -> crate::DaemonSettings {
        self.settings.get()
    }

    /// The configured eval backend with its credential hydrated — `None`
    /// when the pick cannot serve evaluations or still lacks a field, so
    /// eval-backed work degrades instead of spending a doomed call.
    pub(crate) fn resolved_eval(&self) -> Option<waku_protocol::eval::EvalSettings> {
        crate::inference::resolve_eval(&self.settings.get(), &self.inference_secrets)
    }

    pub(crate) fn auto_prompt_turn_is_latest(&self, session_id: Uuid, turn_id: Uuid) -> bool {
        if self.agent.is_working(session_id) {
            return false;
        }
        self.task_state.lock().sessions.iter().any(|session| {
            session.id == session_id
                && session.archived_at.is_none()
                && session.turns.last().is_some_and(|turn| turn.id == turn_id)
                && session.queued_messages.is_empty()
        })
    }

    pub(crate) fn queue_auto_prompt(
        &self,
        session_id: Uuid,
        source_turn: Uuid,
        prompt: String,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        if !self.auto_prompt_turn_is_latest(session_id, source_turn) {
            bail!("task moved on before auto prompt dispatch");
        }
        if self.session_quarantined(session_id) {
            bail!("task is quarantined");
        }
        if !self.sessions.lock().contains_key(&session_id) {
            bail!("task runtime is unavailable");
        }
        self.queue_agent_prompt(session_id, prompt, None, events)
    }

    /// Pop every queued agent prompt for the session, in submission order.
    /// A turn that started working mid-drain holds the remainder for the
    /// provider's finish event.
    pub(super) fn drain_agent_queue(
        &self,
        session_id: Uuid,
        driver: &DriverHandle,
        sink: &EventSink,
    ) -> anyhow::Result<()> {
        rehydrate_agent_queue(&self.agent, &self.task_state, &self.task_store, session_id);
        while let Some(entry) = self.agent.pop_queued(session_id) {
            if (self.agent.is_working(session_id)
                && !(employee_report_interrupts(&entry) && driver.supports_steer()))
                || employee_update_streaming(session_id, entry.sender, &self.agent, &self.boss)
            {
                self.agent.requeue_front(session_id, entry);
                break;
            }
            deliver_agent_prompt(
                session_id,
                driver,
                entry,
                sink,
                &self.agent,
                &self.auto_prompts,
                &self.task_state,
                &self.task_store,
                &self.boss,
                &self.automations,
            )?;
        }
        Ok(())
    }

    /// The running runtime's id for a session, when one is live. Queue
    /// change events go out on that runtime's stream so attached clients
    /// redraw the chip row without waiting for the next save cycle.
    pub(super) fn runtime_id_for(&self, session_id: Uuid) -> Option<Uuid> {
        self.sessions
            .lock()
            .get(&session_id)
            .map(|entry| entry.runtime_id)
    }

    /// Cancel a parked agent prompt — the chip's own remove affordance.
    /// Works whether or not the session's runtime is up: the in-memory
    /// queue and the session document's mirrored entry are both cleared.
    pub(super) fn cancel_queued_prompt(
        &self,
        session_id: Uuid,
        queued_message_id: Uuid,
        events: &EventSink,
    ) -> anyhow::Result<ResponsePayload> {
        self.agent.remove_queued(session_id, queued_message_id);
        {
            let mut state = self.task_state.lock();
            let session = state
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("task {session_id} is unknown to the daemon"))?;
            self.task_store.hydrate(session)?;
            let index = session
                .queued_messages
                .iter()
                .position(|queued| queued.id == queued_message_id);
            if let Some(index) = index {
                if !session.queued_messages[index].is_agent_owned() {
                    bail!(
                        "queued message {queued_message_id} is owned by the client, not the daemon"
                    );
                }
                session.queued_messages.remove(index);
                session.updated_at = crate::model::unix_time();
                state.mark_session_dirty(session_id);
                self.task_store.save(&mut state)?;
            }
            // The chip may already have been cleared when delivery began;
            // clients can still send its in-flight remove request afterward.
        }
        if let Some(runtime_id) = self.runtime_id_for(session_id) {
            send_agent_queue_changed(
                &self.task_state,
                &events.for_session(session_id, runtime_id),
                session_id,
            );
        }
        Ok(ResponsePayload::Ack)
    }
}
