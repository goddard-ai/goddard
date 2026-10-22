use super::*;

/// Remove a session's runtime entry under its ownership gate — the shared
/// body of `WakuBackend::remove_runtime`, also driven from detached
/// teardown threads that have no backend handle.
pub(super) fn remove_runtime_entry(
    lifecycles: &RuntimeLifecycles,
    sessions: &Mutex<HashMap<Uuid, RuntimeEntry>>,
    agent: &crate::agent::AgentState,
    repo_maps: &Arc<(Mutex<RepoMaps>, Condvar)>,
    session_id: Uuid,
    expected: Option<Uuid>,
    clear: bool,
) -> Option<RuntimeEntry> {
    let lifecycle = lifecycles.session(session_id);
    let owner = lifecycle.lock();
    let mut sessions = sessions.lock();
    let current = sessions
        .get(&session_id)
        .map(|entry| entry.runtime_id)
        .or(owner.get());
    if expected.is_some_and(|id| current != Some(id)) {
        return None;
    }
    owner.set(None);
    let removed = sessions.remove(&session_id);
    drop(sessions);
    if clear {
        agent.clear_session(session_id);
    } else {
        agent.revoke_session(session_id);
        agent.note_driver_event(session_id, &DriverEvent::ProcessExited);
        agent.take_daemon_interrupt(session_id);
    }
    repo_maps.0.lock().sessions.remove(&session_id);
    if let Some(entry) = &removed {
        entry.driver.begin_shutdown();
    }
    removed
}

impl WakuBackend {
    /// Start a provider runtime for `session_id` and forward its events into
    /// `events`, which must already target the new `runtime_id`.
    ///
    /// Shared by client `Start` requests and the agent commands' cold start:
    /// both mint the session's scoped credential (when the daemon's agent
    /// tools are enabled) and both run the forwarder that tracks turns,
    /// drains queued agent prompts, and attributes accepted steers.
    pub(super) fn spawn_runtime(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        provider: ProviderKind,
        options: DriverStartOptions,
        events: EventSink,
    ) -> anyhow::Result<(DriverHandle, bool)> {
        let lifecycle = self.runtime_lifecycles.session(session_id);
        let owner = lifecycle.lock();
        let result = (|| {
            self.memory_guard.check_admission(session_id)?;
            anyhow::ensure!(
                !self
                    .runtime_lifecycles
                    .shutting_down
                    .load(std::sync::atomic::Ordering::Acquire),
                "daemon is shutting down"
            );
            anyhow::ensure!(
                !self.removed_session_ids.lock().contains(&session_id),
                "task {session_id} was removed"
            );
            drop_detached(self.remove_runtime(session_id, None, false));
            owner.set(Some(runtime_id));
            self.spawn_runtime_inner(session_id, runtime_id, provider, options, events.clone())
        })();
        if result.is_err() {
            drop_detached(self.remove_runtime(session_id, Some(runtime_id), false));
            events
                .for_session(session_id, runtime_id)
                .end_session_runtime();
        }
        result
    }

    fn spawn_runtime_inner(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        provider: ProviderKind,
        options: DriverStartOptions,
        events: EventSink,
    ) -> anyhow::Result<(DriverHandle, bool)> {
        let (wake, _wake_events) = smol::channel::bounded(1);
        let (event_sender, event_receiver) = driver::event_channel(wake);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let event_sender = event_sender.with_process_anchor(self.memory_guard.anchor(runtime_id));
        let resumable = options.provider_cursor.is_some();
        let cwd = options.cwd.clone();
        let mut options = options;
        self.boss.require_active(session_id)?;
        anyhow::ensure!(
            !self.boss_fenced_chat(session_id),
            "task {session_id} is a fenced Boss generation; the active chat owns coordination"
        );
        if self.boss.is_boss_principal(session_id) {
            self.ensure_coordinator_available(session_id)?;
        }
        let managed = self.boss.is_managed(session_id);
        let employee_snapshot = self.boss.employee(session_id);
        let environment = self
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.environment())
            .unwrap_or_default();
        if managed {
            self.boss
                .set_project_context(session_id, options.cwd.clone());
            // OpenCode registers MCP servers by directory on its shared
            // native service. Other employees retain the ordinary task cwd;
            // sandbox guests already have an isolated provider service.
            if self.boss.is_boss_principal(session_id)
                || (provider == ProviderKind::OpenCode && !environment.is_sandbox())
            {
                options.cwd = self.boss.workspace(session_id)?;
            }
            self.boss.reset_context(session_id);
            options.read_own_transcript = true;
            options.computer_use_enabled &= employee_snapshot
                .as_ref()
                .is_some_and(|employee| employee.permissions.computer_use);
        }
        // A projectless workspace is daemon-owned scratch state: its
        // directory can vanish between draft creation and the first prompt —
        // emptied trash, an archive sweep, a client that provisioned it on a
        // stale listing. Restore unzips its archive or recreates it empty
        // instead of failing the launch on a path the user never manages.
        // Any other missing cwd is a real problem and fails here with the
        // path named, not inside the provider's spawn error.
        if !options.cwd.is_dir() && crate::projectless::is_projectless_path(&options.cwd) {
            crate::projectless::restore_workspace(&options.cwd).with_context(|| {
                format!(
                    "could not recreate the task's projectless workspace {}",
                    options.cwd.display()
                )
            })?;
        }
        if !options.cwd.is_dir() {
            bail!(
                "the task's working directory does not exist: {}",
                options.cwd.display()
            );
        }
        // The credential exists before the process does so it can travel
        // with the runtime's launch environment. A missing CLI or unset
        // daemon address disables injection for this launch only. Either
        // agent surface — task tools or settings writes — gets it injected,
        // as does a session whose own transcript is meant to be read back
        // (a provider-switch handoff or a side chat): its scoped read works
        // without the cross-task surface.
        let daemon_settings = self.settings.get();
        // Computer Use is experimental: the enable flag only counts while the
        // experiment opt-in is on, whatever a client or a hand-edited settings
        // document sent over the wire.
        options.computer_use_enabled = crate::computer_use::resolve_enabled(
            options.computer_use_enabled,
            daemon_settings.computer_use_experiment_enabled,
        );
        // A cloud task runs on the provider's hosted environment — nothing
        // local executes, so the launch injections below (agent surface,
        // subagents, workspace code-map index, integrations) would all point
        // at a host the remote side cannot see. They stay off for cloud launches.
        let cloud_launch = environment.is_cloud();
        // The native helper and the REPL approval channel live on this host.
        // A cloud process or sandbox guest cannot reach either one directly.
        options.computer_use_enabled &= !cloud_launch && !environment.is_sandbox();
        if managed && cloud_launch {
            bail!("Boss roles require a provider runtime that can reach this daemon");
        }
        if !cloud_launch {
            match self.agent_launch_env(session_id) {
                Ok(launch) => {
                    #[cfg(test)]
                    if let Some(observe) = &self.runtime_test_env {
                        observe(&launch);
                    }
                    options.agent = Some(launch);
                }
                Err(error) if managed => {
                    return Err(error.context("Boss requires its scoped agent surface"));
                }
                Err(error) => {
                    eprintln!(
                        "goddard-daemon: agent surface unavailable for session {session_id}: {error:#}"
                    );
                    // Without the launch env no `goddard-agent` exists to
                    // report this, so the transcript carries the reason.
                    let detail = format!("{error:#}");
                    let pair = localized!("errors.agent_surface_unavailable", error = &detail);
                    if let Ok(wire) = event_to_wire(DriverEvent::localized_notice(pair)) {
                        let _ = events.send(wire);
                    }
                }
            }
        }
        // Named subagents ride the launch with the runtime: the fixed roster
        // resolves its models through the same class map routing uses, so
        // routing and subagents share one user-editable map. Do not hand a
        // no-op driver the roster: that makes an enabled experiment look
        // successful while the provider silently ignores it.
        if !managed && !cloud_launch && daemon_settings.subagents_enabled {
            match crate::subagents::support_for(provider) {
                crate::subagents::SupportLevel::Unsupported => {
                    if let Ok(wire) = event_to_wire(DriverEvent::localized_error(localized!(
                        "errors.subagents_unsupported_provider",
                        provider = provider.display_name()
                    ))) {
                        let _ = events.send(wire);
                    }
                }
                crate::subagents::SupportLevel::Supported
                | crate::subagents::SupportLevel::Advisory => {
                    options.subagents = Some(crate::subagents::spec_for(
                        provider,
                        daemon_settings.provider_route_classes.get(&provider),
                        &daemon_settings.route_classes,
                    ));
                }
            }
        }
        // Auto-mode permission review rides the same credential-backed
        // evaluation provider as routing. Unconfigured leaves each driver's
        // ask-the-user path — a provider missing its credential would only
        // fail closed there too, so it is withheld the same way rather than
        // spending a doomed call and a decision-log row on every request.
        options.eval = crate::inference::resolve_eval(&daemon_settings, &self.inference_secrets);
        // Keep one background index per local workspace warm so the agent's
        // first precise map request can return immediately. The index itself
        // stays out of the prompt unless the agent asks for it.
        if !cloud_launch && options.agent.is_some() {
            let build_index = {
                let mut maps = self.repo_maps.0.lock();
                maps.sessions.insert(session_id, options.cwd.clone());
                !maps.indexes.contains_key(&options.cwd)
            };
            if build_index {
                spawn_repo_map_refresh(&self.repo_maps, options.cwd.clone());
            }
        }
        // Connected integrations keep their provider-neutral MCP descriptions.
        // Computer Use is owned separately by the task-scoped CLI service.
        options.mcp_servers = if managed {
            let grants = employee_snapshot
                .as_ref()
                .map(|employee| employee.permissions.integration_ids.clone())
                .unwrap_or_default();
            if crate::integrations::deliver::uses_acp(provider)
                && !self.integrations.http_mcp_supported(provider)
                && (!daemon_settings.integrations.is_empty()
                    || daemon_settings.mcp_servers.iter().any(|server| {
                        matches!(
                            server.transport,
                            waku_protocol::integrations::McpServerTransport::Http { .. }
                        )
                    }))
            {
                bail!(
                    "this provider cannot yet enforce persona-scoped MCP access; choose a provider with session MCP support"
                );
            }
            self.integrations.scoped_mcp_servers(session_id, &grants)
        } else if cloud_launch {
            Vec::new()
        } else {
            self.integrations.launch_mcp_servers(provider)
        };
        options.http_mcp_capability_recorder = if crate::integrations::deliver::uses_acp(provider) {
            let integrations = self.integrations.clone();
            Some(Arc::new(move |supported| {
                if let Err(error) = integrations.record_http_mcp_capability(provider, supported) {
                    eprintln!(
                        "goddard-mcp: could not record {} HTTP capability: {error:#}",
                        provider.display_name()
                    );
                }
            }) as Arc<dyn Fn(bool) + Send + Sync>)
        } else {
            None
        };
        let prepared_plan = self
            .prepare_plan_delivery(
                session_id,
                runtime_id,
                provider,
                &options,
                employee_snapshot.as_ref(),
            )
            .map_err(|error| {
                if let Some(binding) = employee_snapshot
                    .as_ref()
                    .filter(|employee| {
                        employee.assignment_kind
                            == waku_protocol::boss::PlanAssignmentKind::Implementation
                    })
                    .and_then(|employee| employee.plan_binding.clone())
                {
                    let failure = super::plan_delivery::classify_plan_failure(&error);
                    if let Err(record) = self.boss.record_plan_readiness(
                        session_id,
                        waku_protocol::boss::PlanReadiness {
                            binding,
                            attempt: runtime_id,
                            runtime_generation: runtime_id,
                            phase: waku_protocol::boss::PlanReadinessPhase::Blocked,
                            delivered: false,
                            acknowledged: false,
                            failure: Some(failure),
                        },
                        true,
                    ) {
                        eprintln!("could not record required-plan preparation blocker: {record:#}");
                    }
                }
                error
            })?;
        // A sandboxed session runs its provider inside a shuru VM — never on
        // the host. Every setup failure fails the task rather than silently
        // falling back to a local process.
        if environment.is_sandbox() {
            if !daemon_settings.sandbox_experiment_enabled {
                bail!(
                    "this task was created with the Sandbox VM environment, but the sandbox experiment is off"
                );
            }
            let launch = crate::sandbox::launch_for_provider(
                provider,
                &options.cwd,
                &self.data_dir,
                |status| {
                    // Launch progress is ephemeral — it exists to name the
                    // Connecting phase, never to enter the transcript.
                    if let Ok(wire) = event_to_wire(DriverEvent::SandboxSetup(status)) {
                        let _ = events.send_ephemeral(wire);
                    }
                },
            )
            .context("could not prepare the sandbox VM")?;
            options.binary = launch.binary;
            options.cwd = launch.cwd;
            options.sandbox = Some(launch.vm);
            // The agent surface's daemon address is this host's loopback —
            // unreachable from inside the guest. The headless computer-use
            // bridge is host-side too, so it is off in the sandbox.
            options.agent = None;
            options.computer_use_enabled = false;
        }
        let computer_use_start_error = if options.computer_use_enabled {
            match driver::ComputerUseRuntime::start(event_sender.clone()) {
                Ok(mut runtime) => {
                    runtime.bind_task(session_id, &options.cwd, self.task_store.blobs());
                    options.computer_use_runtime = Some(runtime);
                    None
                }
                Err(error) => {
                    options.computer_use_enabled = false;
                    Some(error)
                }
            }
        } else {
            None
        };
        // The agent-surface instruction reaches the session through whichever
        // channel its provider offers; first-prompt context needs the launch's
        // scopes for drivers without a native channel. A failed start revokes
        // the credential and this record together.
        if let Some(agent_env) = &options.agent {
            self.agent.note_surface(session_id, agent_env.scope());
        }
        // A launch that never came up keeps no credential.
        let sandboxed_launch = options.sandbox.is_some();
        let (handle, stable) = self.start_runtime_driver(
            session_id,
            runtime_id,
            provider,
            options,
            event_sender,
            cloud_launch,
        )?;
        if prepared_plan.is_some()
            && handle.agent_surface_delivery() == driver::AgentSurfaceDelivery::Absent
        {
            handle.begin_shutdown();
            bail!("plan delivery unsupported: adapter omitted scoped control transport");
        }
        let plan_gate = prepared_plan.map(|prepared| {
            prepared.attach(
                handle.clone(),
                events.clone(),
                self.runtime_lifecycles.session(session_id),
                runtime_id,
            )
        });
        // A recovered attempt's withheld briefs deliver through the verified
        // boundary immediately — they must not wait behind a fresh prompt.
        if let Some(gate) = &plan_gate {
            gate.resume_pending();
        }
        let handle = plan_gate.as_ref().map_or(handle.clone(), |gate| {
            DriverHandle::from_control(gate.clone())
        });
        let handle = self.boss_driver_boundary(session_id, runtime_id, handle);
        let computer_use_available = handle.computer_use_available();
        if let Some(error) = computer_use_start_error {
            let error = format!("{error:#}");
            let pair = localized!("errors.computer_use_start_failed", error = &error);
            if let Ok(wire) = event_to_wire(DriverEvent::localized_notice(pair)) {
                let _ = events.send(wire);
            }
        }
        if sandboxed_launch {
            // The provider process is up — clear the launch phase so the
            // transcript's indicator falls back to its ordinary working state.
            if let Ok(wire) = event_to_wire(DriverEvent::SandboxSetup(
                waku_protocol::model::SandboxSetupStatus::Ready,
            )) {
                let _ = events.send_ephemeral(wire);
            }
        }
        self.register_runtime_forwarder(
            session_id,
            RuntimeEntry {
                plan_gate,
                runtime_id,
                driver: handle.clone(),
                stable,
                last_active: std::time::Instant::now(),
                resumable,
                computer_use_available,
                provider,
                cwd,
            },
            event_receiver,
            events,
        )?;
        Ok((handle, computer_use_available))
    }

    fn start_runtime_driver(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        provider: ProviderKind,
        options: DriverStartOptions,
        sender: driver::DriverEventSender,
        cloud: bool,
    ) -> anyhow::Result<(DriverHandle, Option<crate::stable_runtime::StableLink>)> {
        #[cfg(test)]
        if let Some(start) = &self.runtime_test_start {
            return start(provider, options, sender).map(|handle| (handle, None));
        }
        if let Some(root) = self.stable_runtime_root(provider) {
            // The opt-in seam: this launch attaches through the surviving
            // runtime host. Every other provider keeps the legacy
            // in-process driver until Stage 4.
            #[cfg(not(unix))]
            anyhow::bail!(
                "stable runtime requires a Unix host (requested {})",
                root.display()
            );
            #[cfg(unix)]
            {
                let (handle, stable) = crate::stable_runtime::start_link(
                    self,
                    &self.task_store,
                    session_id,
                    runtime_id,
                    provider,
                    &options,
                    sender,
                    &root,
                    None,
                )?;
                return Ok((handle, Some(stable)));
            }
        }
        let handle = if cloud {
            driver::start_cloud(provider, options, sender)?
        } else {
            driver::start_local(provider, options, sender)?
        };
        Ok((handle, None))
    }

    /// Revoke only the owner being removed. A launch attempt can own credentials
    /// before it owns a driver entry; delayed cleanup must cover that case too.
    pub(super) fn remove_runtime(
        &self,
        session_id: Uuid,
        expected: Option<Uuid>,
        clear: bool,
    ) -> Option<RuntimeEntry> {
        remove_runtime_entry(
            &self.runtime_lifecycles,
            &self.sessions,
            &self.agent,
            &self.repo_maps,
            session_id,
            expected,
            clear,
        )
    }

    /// Publish the runtime before any buffered driver event can be consumed.
    pub(super) fn register_runtime_forwarder(
        &self,
        session_id: Uuid,
        entry: RuntimeEntry,
        event_receiver: crossbeam_channel::Receiver<DriverEvent>,
        events: EventSink,
    ) -> anyhow::Result<std::thread::JoinHandle<()>> {
        let lifecycle = self.runtime_lifecycles.session(session_id);
        let owner = lifecycle.lock();
        let runtime_id = entry.runtime_id;
        owner.set(Some(runtime_id));
        let forwarder_handle = entry.driver.clone();
        let agent = self.agent.clone();
        let task_state = self.task_state.clone();
        let task_store = self.task_store.clone();
        let sessions = self.sessions.clone();
        let lifecycles = self.runtime_lifecycles.clone();
        let automations = self.automations.clone();
        let boss = self.boss.clone();
        let auto_prompts = self.auto_prompts.clone();
        let repo_maps = self.repo_maps.clone();
        let data_dir = self.data_dir.clone();
        let retirement_events = events.clone();
        // Hold registration through thread creation. The forwarder's first
        // sessions lock cannot succeed until its entry is visible, even when
        // the provider has already buffered Connected or ProcessExited.
        let mut registered = self.sessions.lock();
        let previous = registered.insert(session_id, entry);
        if let Some(previous) = &previous {
            previous.driver.begin_shutdown();
        }
        drop_detached(previous);
        let name = format!("goddard-daemon-events-{session_id}");
        let forward = move || {
            forward_driver_events(
                session_id,
                runtime_id,
                event_receiver,
                events,
                forwarder_handle,
                agent,
                task_state,
                task_store,
                sessions,
                lifecycles,
                automations,
                boss,
                auto_prompts,
                repo_maps,
                data_dir,
            );
        };
        #[cfg(test)]
        let forwarding = match &self.runtime_test_forward {
            Some(spawn) => spawn(name, Box::new(forward)),
            None => std::thread::Builder::new().name(name).spawn(forward),
        };
        #[cfg(not(test))]
        let forwarding = std::thread::Builder::new().name(name).spawn(forward);
        let forwarding = forwarding.context("could not start daemon event forwarding thread");
        drop(registered);
        if forwarding.is_err() {
            drop_detached(self.remove_runtime(session_id, Some(runtime_id), false));
            retirement_events
                .for_session(session_id, runtime_id)
                .end_session_runtime();
        }
        forwarding
    }

    /// Mint the runtime's scoped credential and assemble the environment the
    /// provider launch receives. The shim directory lives beside the daemon
    /// database so shared-service providers have somewhere private to put a
    /// per-session launcher.
    pub(super) fn agent_launch_env(
        &self,
        session_id: Uuid,
    ) -> anyhow::Result<crate::agent::AgentLaunchEnv> {
        let daemon_address = self
            .daemon_address
            .lock()
            .clone()
            .ok_or_else(|| anyhow!("the daemon's bound address is unknown"))?;
        #[cfg(test)]
        let cli_path = match &self.runtime_test_cli {
            Some(path) => path.clone(),
            None => crate::agent::agent_cli_path()?,
        };
        #[cfg(not(test))]
        let cli_path = crate::agent::agent_cli_path()?;
        let shim_directory = self
            .task_store
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("agent")
            .join(session_id.to_string());
        let settings = self.settings.get();
        // `side_chat_of` is a list column, so a skeleton answers this
        // without a hydrate.
        let parent_task_id = self
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .and_then(|session| session.side_chat_of);
        Ok(crate::agent::AgentLaunchEnv {
            token: self.agent.mint(session_id),
            task_id: session_id,
            parent_task_id,
            daemon_address,
            cli_path,
            shim_directory,
            task_tools: settings.agent_tools_enabled || self.boss.is_managed(session_id),
            settings_writes: settings.agent_settings_enabled && !self.boss.is_managed(session_id),
            boss: self.boss.is_boss_principal(session_id),
            // `memory` rides the bucket store the Boss experiment owns, and
            // only a session with a real registered project has a shared
            // bucket to reach — Boss principals keep their own channel.
            memory: settings.boss_experiment_enabled
                && !self.boss.is_boss_principal(session_id)
                && self.session_memory_project(session_id).is_some(),
            resource_reservation: self
                .boss
                .employee(session_id)
                .and_then(|employee| employee.ticket)
                .and_then(|ticket| ticket.reservation),
        })
    }

    /// Return the live driver for `session_id`, cold-starting it from the
    /// stored task's provider cursor and saved options when no runtime is
    /// running.
    pub(crate) fn ensure_agent_runtime(
        &self,
        session_id: Uuid,
        events: &EventSink,
    ) -> anyhow::Result<(Uuid, DriverHandle)> {
        self.memory_guard.check_admission(session_id)?;
        let lifecycle = self.runtime_lifecycles.session(session_id);
        let _owner = lifecycle.lock();
        anyhow::ensure!(
            !self
                .runtime_lifecycles
                .shutting_down
                .load(std::sync::atomic::Ordering::Acquire),
            "daemon is shutting down"
        );
        if let Some(entry) = self.sessions.lock().get_mut(&session_id) {
            entry.last_active = std::time::Instant::now();
            return Ok((entry.runtime_id, entry.driver.clone()));
        }
        let (provider, options) = {
            let mut state = self.task_state.lock();
            let index = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("task {session_id} is unknown to the daemon"))?;
            self.task_store
                .hydrate(&mut state.sessions[index])
                .context("could not load the task's stored state")?;
            let session = &state.sessions[index];
            anyhow::ensure!(
                session.archived_at.is_none(),
                "task {session_id} is archived"
            );
            let cwd = session
                .workspace
                .path()
                .map(Path::to_path_buf)
                .or_else(|| {
                    state
                        .projects
                        .iter()
                        .find(|project| project.id == session.project_id)
                        .map(|project| project.path.clone())
                })
                .ok_or_else(|| anyhow!("task {session_id} has no project to run in"))?;
            let provider = session.provider;
            let options = DriverStartOptions {
                binary: self.provider_binary(provider)?,
                cwd,
                mode: session.runtime_mode,
                model: session.model.clone(),
                reasoning_effort: session.reasoning_effort.clone(),
                service_tier: session.service_tier.clone(),
                context_window: session.context_window.clone(),
                agent_preset: session.agent_preset.clone(),
                computer_use_enabled: self.settings.get().computer_use_enabled,
                agent: None,
                // A switched or side-chat task cold-started this way still
                // gets the read surface — the session is hydrated here.
                read_own_transcript: session.side_chat_of.is_some()
                    || !session.suspended_provider_sessions.is_empty()
                    || session.pending_provider_context.is_some(),
                subagents: None,
                computer_use_runtime: None,
                mcp_servers: Vec::new(),
                http_mcp_capability_recorder: None,
                provider_cursor: session.provider_cursor.clone(),
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
                ephemeral: false,
            };
            (provider, options)
        };
        let runtime_id = Uuid::new_v4();
        let sink = events.begin_session_runtime(session_id, runtime_id);
        let (handle, _) =
            match self.spawn_runtime(session_id, runtime_id, provider, options, sink.clone()) {
                Ok(launch) => launch,
                Err(error) => {
                    sink.end_session_runtime();
                    return Err(error);
                }
            };
        Ok((runtime_id, handle))
    }
}
