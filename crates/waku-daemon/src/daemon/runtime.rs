use super::*;

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
        let (wake, _wake_events) = smol::channel::bounded(1);
        let (event_sender, event_receiver) = driver::event_channel(wake);
        let mut options = options;
        self.boss.require_active(session_id)?;
        let managed = self.boss.is_managed(session_id);
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
            options.computer_use_enabled &= self
                .boss
                .employee(session_id)
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
                Ok(launch) => options.agent = Some(launch),
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
            let grants = self
                .boss
                .employee(session_id)
                .map(|employee| employee.permissions.integration_ids)
                .unwrap_or_default();
            if crate::integrations::deliver::uses_acp(provider)
                && !self.integrations.http_mcp_supported(provider)
                && !daemon_settings.integrations.is_empty()
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
        let handle = match if cloud_launch {
            driver::start_cloud(provider, options, event_sender)
        } else {
            driver::start_local(provider, options, event_sender)
        } {
            Ok(handle) => handle,
            Err(error) => {
                self.agent.revoke_session(session_id);
                return Err(error);
            }
        };
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
        let forwarder_handle = handle.clone();
        let agent = self.agent.clone();
        let task_state = self.task_state.clone();
        let task_store = self.task_store.clone();
        let sessions = self.sessions.clone();
        let automations = self.automations.clone();
        let boss = self.boss.clone();
        let auto_prompts = self.auto_prompts.clone();
        let repo_maps = self.repo_maps.clone();
        std::thread::Builder::new()
            .name(format!("goddard-daemon-events-{session_id}"))
            .spawn(move || {
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
                    automations,
                    boss,
                    auto_prompts,
                    repo_maps,
                );
            })
            .context("could not start daemon event forwarding thread")?;
        Ok((handle, computer_use_available))
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
    pub(super) fn ensure_agent_runtime(
        &self,
        session_id: Uuid,
        events: &EventSink,
    ) -> anyhow::Result<(Uuid, DriverHandle)> {
        if let Some(entry) = self.sessions.lock().get_mut(&session_id) {
            entry.last_active = std::time::Instant::now();
            return Ok((entry.runtime_id, entry.driver.clone()));
        }
        // A per-session lock keeps two simultaneous agent prompts from
        // cold-starting the same stored task twice.
        let start_lock = self
            .runtime_start_locks
            .lock()
            .entry(session_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _start_guard = start_lock.lock();
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
        let resumable = options.provider_cursor.is_some();
        let cwd = options.cwd.clone();
        let (handle, computer_use_available) =
            match self.spawn_runtime(session_id, runtime_id, provider, options, sink.clone()) {
                Ok(launch) => launch,
                Err(error) => {
                    sink.end_session_runtime();
                    return Err(error);
                }
            };
        let driver = handle.clone();
        self.sessions.lock().insert(
            session_id,
            RuntimeEntry {
                runtime_id,
                driver: handle,
                last_active: std::time::Instant::now(),
                resumable,
                computer_use_available,
                provider,
                cwd,
            },
        );
        Ok((runtime_id, driver))
    }
}
