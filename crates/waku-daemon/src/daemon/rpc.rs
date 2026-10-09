use super::*;

impl Backend for WakuBackend {
    fn authenticate_agent(&self, token: &str) -> Option<Uuid> {
        self.agent.resolve(token)
    }

    fn authenticate_paired(&self, token: &str) -> bool {
        self.pairing.authenticate(token)
    }

    fn request_pair(&self, device_name: &str, transport: &str) -> crate::pairing::PairReply {
        self.pairing.request_blocking(device_name, transport)
    }

    fn set_pairing_sink(&self, sink: crate::pairing::PairingSink) {
        self.pairing.set_sink(sink);
    }

    fn daemon_name(&self) -> String {
        self.our_name.clone()
    }

    fn lan_advertisement(&self) -> Option<(String, String)> {
        // The EndpointId doubles as the daemon's LAN identity — the same
        // string friend codes and iroh discovery carry.
        let id = waku_share::identity::load_or_create(&self.share_dir())
            .map(|secret| secret.public().to_string())
            .ok()?;
        Some((self.our_name.clone(), id))
    }

    fn note_exposed_port(&self, port: Option<u16>) {
        *self.exposed_port.lock() = port;
    }

    fn kickstart_reachability(&self) {
        self.share.kickstart();
    }

    fn set_friends_sink(&self, sink: crate::share::FriendsSink) {
        self.share.set_sink(sink);
    }

    fn set_task_state_sink(&self, sink: crate::share::TaskNotifier) {
        *self.task_notifier.lock() = Some(sink.clone());
        self.share.set_task_notifier(sink.clone());
        self.boss.set_task_notifier(sink.clone());
        self.automations.set_task_notifier(sink);
    }

    fn set_automations_sink(&self, sink: crate::automations::AutomationsSink) {
        self.automations.set_sink(sink);
    }

    fn set_event_source(&self, events: EventSink) {
        *self.event_source.lock() = events.clone();
        self.boss.recover_interrupted();
        self.automations.set_event_source(events.clone());
        self.auto_prompts.set_event_source(events.clone());
        // The reaper starts with the event hub: retiring a runtime needs a
        // sink, and before serve() installs one there is nothing to retire.
        if self
            .idle_reaper_started
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        // The stats sampler shares the guard: it owns a file and a thread,
        // so a second event-source install must not duplicate it either.
        // The probe locks each map in turn rather than nesting — per-minute
        // cadence means a consistent-as-of snapshot per map is enough.
        let probe_sessions = self.sessions.clone();
        let probe_terminals = self.terminals.clone();
        let probe_state = self.task_state.clone();
        self.stats.spawn_sampler(move || {
            let (runtimes, runtime_dirs, running) = {
                let sessions = probe_sessions.lock();
                let running: HashSet<Uuid> = sessions.keys().copied().collect();
                let dirs = sessions
                    .iter()
                    .map(|(session_id, entry)| crate::stats::RuntimeDir {
                        session_id: *session_id,
                        provider: entry.provider,
                        cwd: std::fs::canonicalize(&entry.cwd)
                            .unwrap_or_else(|_| entry.cwd.clone()),
                    })
                    .collect();
                (sessions.len() as u32, dirs, running)
            };
            let (terminals, terminal_roots) = {
                let terminals = probe_terminals.lock();
                let roots = terminals
                    .iter()
                    .map(|(_, entry)| (entry.terminal.child_pid(), entry.owner))
                    .collect();
                (terminals.len() as u32, roots)
            };
            let state = probe_state.lock();
            crate::stats::StatsProbe {
                runtimes,
                terminals,
                runtime_dirs,
                terminal_roots,
                sessions_total: state.sessions.len() as u32,
                sessions: state
                    .sessions
                    .iter()
                    .filter_map(|session| {
                        session_stats_sample(session, running.contains(&session.id))
                    })
                    .collect(),
            }
        });
        let sessions = self.sessions.clone();
        let task_state = self.task_state.clone();
        let settings = self.settings.clone();
        let agent = self.agent.clone();
        let boss = self.boss.clone();
        let repo_maps = self.repo_maps.clone();
        // A platform memory-pressure signal, when this OS has one, lets the
        // reaper shed safely evictable runtimes ahead of the age cutoff —
        // before contention becomes a daemon restart. `None` on platforms
        // without a signal or when setup failed keeps the ordinary cadence.
        let pressure = crate::pressure::watch();
        let _ = std::thread::Builder::new()
            .name("waku-idle-reaper".into())
            .spawn(move || {
                let retire_employees = || {
                    let now = crate::model::unix_time();
                    if let Err(error) = boss.retire_expired(now) {
                        eprintln!("could not retire finished Boss employees: {error:#}");
                    }
                    if let Err(error) = boss.archive_graced_plans(now) {
                        eprintln!("could not archive graced Boss plans: {error:#}");
                    }
                };
                // A pressure shed pass just ran — shed at most once per
                // cooldown so a persistent-pressure stream cannot churn
                // through teardown-and-reopen cycles.
                let mut last_pressure_shed: Option<std::time::Instant> = None;
                loop {
                    let shed = |under_pressure: bool| {
                        for (session_id, runtime_id, driver) in reap_idle_runtimes(
                            &sessions,
                            &task_state,
                            &settings,
                            &agent,
                            under_pressure,
                        ) {
                            evict_idle_runtime(
                                session_id, runtime_id, driver, &agent, &repo_maps, &events,
                            );
                        }
                    };
                    let Some(pressure) = &pressure else {
                        std::thread::sleep(IDLE_REAPER_INTERVAL);
                        shed(false);
                        retire_employees();
                        continue;
                    };
                    crossbeam_channel::select! {
                        recv(pressure) -> _ => {
                            if last_pressure_shed.is_none_or(|at| {
                                at.elapsed() >= PRESSURE_SHED_COOLDOWN
                            }) {
                                last_pressure_shed = Some(std::time::Instant::now());
                                shed(true);
                            }
                            retire_employees();
                        }
                        recv(crossbeam_channel::after(IDLE_REAPER_INTERVAL)) -> _ => {
                            shed(false);
                            retire_employees();
                        }
                    }
                }
            });
    }

    fn set_review_notifier(&self, notifier: crate::share::ReviewNotifier) {
        self.share.set_review_notifier(notifier);
    }

    fn set_session_streamer(&self, streamer: crate::share::SessionStreamer) {
        self.share.set_session_streamer(streamer);
    }

    fn set_friend_session_sink(&self, sink: crate::share::FriendSessionSink) {
        self.share.set_friend_session_sink(sink);
    }

    fn trigger_automation_webhook(
        &self,
        automation_id: Uuid,
        key: &str,
    ) -> anyhow::Result<Option<waku_protocol::automations::AutomationRun>> {
        self.automations.trigger_webhook(automation_id, key)
    }

    fn handle(
        &self,
        request: Request,
        events: EventSink,
        agent: Option<Uuid>,
    ) -> anyhow::Result<ResponsePayload> {
        let session_id = request.session_id;
        let runtime_id = request.runtime_id;
        if let Command::Respond { request_id, .. } = &request.command
            && request_id.starts_with(waku_protocol::PLAN_FINALIZE_REQUEST_PREFIX)
        {
            anyhow::ensure!(
                agent.is_none(),
                "only a human client can answer plan approval"
            );
            anyhow::ensure!(
                self.sessions
                    .lock()
                    .get(&session_id)
                    .is_some_and(|entry| entry.runtime_id == runtime_id),
                "plan approval belongs to a different or stopped runtime"
            );
        }
        let boss_chat_command = matches!(
            &request.command,
            Command::Start { .. } | Command::Prompt { .. }
        ) && {
            let identity = self.boss.identity_and_session().0.id;
            self.task_state.lock().sessions.iter().any(|session| {
                session.id == session_id
                    && session.boss_managed
                    && session.project_id == identity
                    && session.planning.is_none()
            })
        };
        let _boss_chat_guard = boss_chat_command.then(|| self.boss.operation_guard());
        if boss_chat_command {
            anyhow::ensure!(
                self.task_state
                    .lock()
                    .sessions
                    .iter()
                    .any(|session| { session.id == session_id && session.archived_at.is_none() }),
                "Boss chat is archived; reopen the active Boss chat"
            );
        }
        match request.command {
            Command::Boss { operation } => Ok(ResponsePayload::Boss {
                result: self.handle_boss_operation(agent, operation, &events)?,
            }),
            Command::ClaimManagedGoalTurn { goal_id, turn_id } => {
                let mut claims = self.managed_goal_claims.lock();
                let claimed = claim_managed_goal_turn(&mut claims, session_id, goal_id, turn_id);
                Ok(ResponsePayload::ManagedGoalTurnClaimed { claimed })
            }
            Command::AttachSession => {
                let sessions = self.sessions.lock();
                let Some(entry) = sessions.get(&session_id) else {
                    return Ok(ResponsePayload::SessionRuntime {
                        runtime_id: None,
                        supports_steer: false,
                        supports_user_input_actions: false,
                    });
                };
                Ok(ResponsePayload::SessionRuntime {
                    runtime_id: Some(entry.runtime_id),
                    supports_steer: entry.driver.supports_steer(),
                    supports_user_input_actions: entry.driver.supports_user_input_actions(),
                })
            }
            Command::GetSettings => Ok(ResponsePayload::Settings {
                settings: self.settings.get(),
            }),
            Command::GetDaemonStats => {
                let (current, previous_boot, previous_boot_clean) = self.stats.snapshot();
                Ok(ResponsePayload::DaemonStats {
                    current,
                    previous_boot,
                    previous_boot_clean,
                })
            }
            Command::GetFriends => {
                // Reading friends state means this install wants to be
                // reachable — incoming requests and offers can only
                // arrive while the endpoint is up.
                self.share.kickstart();
                Ok(ResponsePayload::Friends {
                    state: self.share.state(),
                })
            }
            Command::GetPairing => Ok(ResponsePayload::Pairing {
                state: self.pairing.state(),
            }),
            Command::RespondPairRequest { request_id, accept } => {
                self.pairing.respond(request_id, accept)?;
                Ok(ResponsePayload::Ack)
            }
            Command::RevokePairedClient { client_id } => {
                self.pairing.revoke(client_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::GetAutomations => Ok(ResponsePayload::Automations {
                state: self.automations.document(),
            }),
            Command::UpsertAutomation { input } => Ok(ResponsePayload::Automation {
                automation: self.automations.upsert(input)?,
            }),
            Command::RemoveAutomation { automation_id } => {
                self.automations.remove(automation_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::RunAutomationNow { automation_id } => Ok(ResponsePayload::AutomationRun {
                run: self.automations.run_now(automation_id)?,
            }),
            Command::SendFriendRequest { code, name } => {
                self.share.send_friend_request(code, name)?;
                Ok(ResponsePayload::Ack)
            }
            Command::RespondFriendRequest { node_id, accept } => {
                self.share.respond_friend_request(node_id, accept)?;
                Ok(ResponsePayload::Ack)
            }
            Command::WithdrawFriendRequest { node_id } => {
                self.share.withdraw_friend_request(node_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::RemoveFriend { node_id } => {
                self.share.remove_friend(node_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::SendFileToFriend {
                node_id,
                path,
                title,
                note,
            } => {
                self.share.send_file(node_id, path, title, note)?;
                Ok(ResponsePayload::Ack)
            }
            Command::SendMessageToFriend { node_id, text } => {
                self.share.send_chat(node_id, text)?;
                Ok(ResponsePayload::Ack)
            }
            Command::CancelTransfer { transfer_id } => {
                self.share.cancel_transfer(transfer_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ProbeFriend { node_id } => {
                self.share.probe_friend(node_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::SetFriendDisplayName { name } => {
                self.share.set_display_name(name)?;
                Ok(ResponsePayload::Ack)
            }
            Command::SetFriendNickname { node_id, nickname } => {
                self.share.set_friend_nickname(node_id.clone(), nickname)?;
                self.rename_friend_sessions(&node_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ShareProjectWithFriend {
                node_id,
                project_path,
            } => {
                self.share.share_project(node_id, project_path)?;
                Ok(ResponsePayload::Ack)
            }
            Command::UnshareProjectWithFriend {
                node_id,
                origin_url,
            } => {
                self.share.unshare_project(node_id, origin_url)?;
                Ok(ResponsePayload::Ack)
            }
            Command::EnableFriendSync {
                node_id,
                origin_url,
            } => {
                self.share.enable_sync(node_id, origin_url)?;
                Ok(ResponsePayload::Ack)
            }
            Command::DisableFriendSync { link_id } => {
                self.share.disable_sync(link_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::SetFriendSyncConfig {
                link_id,
                auto_push,
                enabled_branches,
            } => {
                self.share
                    .set_sync_config(link_id, auto_push, enabled_branches)?;
                Ok(ResponsePayload::Ack)
            }
            Command::FriendSyncNow { link_id, branch } => {
                self.share.sync_now(link_id, branch)?;
                Ok(ResponsePayload::Ack)
            }
            Command::FriendSyncAlertAction { alert_id, action } => {
                self.share.sync_alert_action(alert_id, action)?;
                Ok(ResponsePayload::Ack)
            }
            Command::GetFriendSyncBranches { link_id } => {
                let (branches, default_branch) = self.share.get_sync_branches(link_id.clone())?;
                Ok(ResponsePayload::FriendSyncBranches {
                    link_id,
                    branches,
                    default_branch,
                })
            }
            Command::SetFriendSessionSharing {
                node_id,
                origin_url,
                enabled,
            } => {
                self.share
                    .set_session_sharing(node_id, origin_url, enabled)?;
                Ok(ResponsePayload::Ack)
            }
            Command::GetFriendSessions {
                node_id,
                origin_url,
            } => {
                let sessions = self.share.friend_sessions(node_id, origin_url)?;
                Ok(ResponsePayload::FriendSessions { sessions })
            }
            Command::WatchFriendSession {
                node_id,
                origin_url,
                session_id,
            } => {
                let session = self
                    .share
                    .watch_friend_session(node_id, origin_url, session_id)?;
                Ok(ResponsePayload::FriendSession {
                    session: Box::new(session),
                })
            }
            Command::UnwatchFriendSession { session_id } => {
                self.share.unwatch_friend_session(session_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::UpdateSettings { settings } => {
                let boss_was_enabled = self.settings.get().boss_experiment_enabled;
                let revoked_app_grant =
                    self.settings
                        .get()
                        .computer_use_allowed_apps
                        .iter()
                        .any(|grant| {
                            grant.verified
                                && !settings
                                    .computer_use_allowed_apps
                                    .iter()
                                    .any(|next| next.verified && next.bundle_id == grant.bundle_id)
                        });
                let computer_use_enabled = crate::computer_use::resolve_enabled(
                    settings.computer_use_enabled,
                    settings.computer_use_experiment_enabled,
                );
                let mut settings = settings;
                // Inference credentials are write-only on the wire: move any
                // submitted keys into the secret store and rebuild the
                // `credential_configured` flags before the document persists
                // or echoes back to clients.
                crate::inference::absorb(&mut settings, &self.inference_secrets);
                self.settings.replace(settings)?;
                if boss_was_enabled && !self.settings.get().boss_experiment_enabled {
                    self.boss.deactivate();
                }
                self.apply_wake_setting();
                crate::integrations::deliver::sync_file_providers(
                    &self.settings.get(),
                    &self.integrations,
                );
                crate::driver::set_computer_use_enabled_for_runtimes(computer_use_enabled);
                if revoked_app_grant {
                    crate::driver::revoke_computer_app_grants_for_runtimes();
                }
                events.settings_changed(self.settings.get());
                Ok(ResponsePayload::Ack)
            }
            Command::UpsertCustomCommand { command } => {
                self.require_agent_settings(agent)?;
                let commands = self.upsert_custom_command(agent, command)?;
                events.settings_changed(self.settings.get());
                Ok(ResponsePayload::CustomCommands { commands })
            }
            Command::RemoveCustomCommand { id, name } => {
                self.require_agent_settings(agent)?;
                let commands = self.remove_custom_command(id, name)?;
                events.settings_changed(self.settings.get());
                Ok(ResponsePayload::CustomCommands { commands })
            }
            Command::ListCustomCommands => {
                self.require_agent_settings(agent)?;
                Ok(ResponsePayload::CustomCommands {
                    commands: self.settings.get().custom_commands,
                })
            }
            Command::ListIntegrations => Ok(ResponsePayload::Integrations {
                snapshots: self.integrations.snapshots(),
            }),
            Command::ConnectIntegration {
                id,
                variant_id,
                providers,
                api_key,
            } => {
                self.integrations
                    .connect(&id, &variant_id, providers, api_key, &events)?;
                crate::integrations::deliver::sync_file_providers(
                    &self.settings.get(),
                    &self.integrations,
                );
                Ok(ResponsePayload::Ack)
            }
            Command::SetIntegrationProviders { id, providers } => {
                self.integrations.set_providers(&id, providers)?;
                crate::integrations::deliver::sync_file_providers(
                    &self.settings.get(),
                    &self.integrations,
                );
                events.settings_changed(self.settings.get());
                Ok(ResponsePayload::Ack)
            }
            Command::DisconnectIntegration { id } => {
                self.integrations.disconnect(&id)?;
                crate::integrations::deliver::sync_file_providers(
                    &self.settings.get(),
                    &self.integrations,
                );
                events.settings_changed(self.settings.get());
                Ok(ResponsePayload::Ack)
            }
            Command::StartIntegrationAuth { id } => {
                self.integrations.begin_auth(&id, &events)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ProbeProvider {
                provider,
                binary_override,
                discover_models,
                probe_version,
            } => {
                if discover_models || probe_version {
                    ensure_shell_environment();
                }
                // The Codex CLI can be updated while its daemon stays open.
                // Refresh its shell lookup at the normal cadence so the next
                // model or version probe can resolve the newly installed CLI.
                if (!discover_models && !probe_version) || provider == ProviderKind::Codex {
                    crate::command_env::refresh_shell_environment_if_stale();
                }
                let mut probe = match binary_override.as_deref() {
                    override_value if discover_models || probe_version => {
                        crate::model::provider_probe(provider, override_value)
                    }
                    override_value => crate::model::cached_provider_probe(provider, override_value),
                };
                let version = probe_version
                    .then(|| {
                        probe
                            .path
                            .as_deref()
                            .and_then(crate::model::probe_provider_version)
                    })
                    .flatten();
                if discover_models {
                    probe = crate::model::discover_provider_models(probe);
                }
                Ok(ResponsePayload::ProviderProbe { probe, version })
            }
            Command::SandboxSignIn { provider } => {
                // The sign-in VM needs the provider checkpoint — build it if
                // this is the provider's first sandboxed touch.
                crate::sandbox::ensure_sign_in_image(provider)?;
                let (program, args, cwd) =
                    crate::sandbox::sign_in_invocation(provider, &self.data_dir)?;
                Ok(ResponsePayload::SandboxSignIn { program, args, cwd })
            }
            Command::SandboxAuthStatus { provider } => Ok(ResponsePayload::SandboxAuthStatus {
                signed_in: crate::sandbox::sandbox_signed_in(provider, &self.data_dir),
            }),
            Command::FetchPlanUsage {
                provider,
                binary_override,
                cli_version,
            } => {
                let usage = match provider {
                    crate::model::ProviderKind::Claude => Some(
                        crate::usage::fetch_claude_plan_usage(cli_version.as_deref())?,
                    ),
                    crate::model::ProviderKind::Codex => {
                        Some(crate::usage::fetch_codex_plan_usage()?)
                    }
                    crate::model::ProviderKind::OpenCode => {
                        crate::usage::fetch_opencode_go_plan_usage()?
                    }
                    crate::model::ProviderKind::Grok => {
                        ensure_shell_environment();
                        let probe = match binary_override.as_deref() {
                            override_value => {
                                crate::model::provider_probe(provider, override_value)
                            }
                        };
                        let binary = probe.path.ok_or_else(|| anyhow!("grok is not installed"))?;
                        Some(crate::usage::fetch_grok_plan_usage(&binary)?)
                    }
                    // These fetchers resolve env tokens and PATH-resolved CLIs
                    // (`gh`, `amp`), so the login-shell environment must be
                    // loaded first — same reason Grok refreshes it.
                    crate::model::ProviderKind::Copilot => {
                        ensure_shell_environment();
                        crate::usage::fetch_copilot_plan_usage()?
                    }
                    crate::model::ProviderKind::Muse => {
                        ensure_shell_environment();
                        crate::usage::fetch_muse_plan_usage()?
                    }
                    crate::model::ProviderKind::Droid => {
                        ensure_shell_environment();
                        crate::usage::fetch_droid_plan_usage()?
                    }
                    crate::model::ProviderKind::Kimi => {
                        ensure_shell_environment();
                        crate::usage::fetch_kimi_plan_usage()?
                    }
                    crate::model::ProviderKind::Cursor => {
                        ensure_shell_environment();
                        crate::usage::fetch_cursor_plan_usage()?
                    }
                    crate::model::ProviderKind::Amp => {
                        // `AMP_API_KEY` never reaches the probe; the CLI is
                        // only resolved when the fetch needs `amp usage`.
                        ensure_shell_environment();
                        crate::usage::fetch_amp_plan_usage(
                            crate::model::provider_probe(provider, binary_override.as_deref())
                                .path
                                .as_deref(),
                        )?
                    }
                    _ => bail!("provider has no plan usage fetcher"),
                };
                Ok(ResponsePayload::PlanUsage { usage })
            }
            Command::ConsumeCodexResetCredit { redeem_request_id } => {
                let Some(_redeem) = self.codex_reset_credit_lock.try_lock() else {
                    bail!("a Codex reset credit redemption is already in flight");
                };
                let outcome = crate::usage::consume_codex_reset_credit(&redeem_request_id)?;
                // Whatever the verdict, the account view may have moved —
                // a spent, missing, or stale credit all surface in the same
                // re-read, which is also how a successful spend confirms.
                let usage = crate::usage::fetch_codex_plan_usage().ok();
                Ok(ResponsePayload::CodexResetCredit { outcome, usage })
            }
            Command::ProbeComputerPermissions { prompt } => {
                // Probing installs and launches the helper app, so it obeys
                // the same experiment opt-in as starting a runtime.
                if !self.settings.get().computer_use_experiment_enabled {
                    bail!("Goddard Computer Use is not enabled in this daemon's settings");
                }
                Ok(ResponsePayload::ComputerPermissions {
                    permissions: crate::computer_use::probe_permissions(prompt)?,
                })
            }
            Command::Evaluate {
                state,
                questions,
                feature,
                timeout_secs,
            } => Ok(ResponsePayload::Evaluation {
                evaluation: evaluate_with_feature(
                    &self.settings,
                    &self.inference_secrets,
                    state,
                    questions,
                    feature.as_deref().unwrap_or("evaluate"),
                    timeout_secs,
                )?,
            }),
            Command::GetWhistleStatus => {
                let (available, downloaded) = crate::whistle::status();
                Ok(ResponsePayload::WhistleStatus {
                    available,
                    downloaded,
                })
            }
            Command::DownloadWhistleModel => {
                crate::whistle::download_model()?;
                let (available, downloaded) = crate::whistle::status();
                Ok(ResponsePayload::WhistleStatus {
                    available,
                    downloaded,
                })
            }
            Command::Transcribe {
                pcm,
                language,
                keywords,
            } => {
                let (text, language, words) = crate::whistle::transcribe(pcm, language, keywords)?;
                Ok(ResponsePayload::Transcription {
                    text,
                    language,
                    words,
                })
            }
            Command::TestEvalConnection { settings } => {
                // The probe's staged fields are unsaved edits — they win over
                // the stored credential, which fills whatever the pane left
                // blank.
                let mut settings = settings;
                crate::inference::hydrate_eval(
                    &mut settings,
                    &self.settings.get(),
                    &self.inference_secrets,
                );
                Ok(ResponsePayload::Evaluation {
                    evaluation: crate::eval::probe(&settings)?,
                })
            }
            Command::GetInferenceCredential { provider } => {
                Ok(ResponsePayload::InferenceCredential {
                    credential: crate::inference::read_credential(
                        &self.inference_secrets,
                        provider,
                    ),
                })
            }
            Command::RouteTask {
                prompt,
                project,
                candidates,
                last_used,
            } => {
                let settings = self.settings.get();
                let eval = crate::inference::resolve_eval(&settings, &self.inference_secrets);
                let run = crate::routing::route_task(
                    eval.as_ref(),
                    &settings.route_classes,
                    &prompt,
                    project.as_deref(),
                    &candidates,
                    last_used.as_ref(),
                );
                crate::eval::append_decision_log(&crate::eval::default_log_path(), &run.record);
                Ok(ResponsePayload::RouteDecision {
                    decision: run.decision,
                })
            }
            Command::RecordRouteOverride { session_id, target } => {
                let mut record = crate::eval::EvalDecisionRecord::empty("route-override");
                record.session_id = Some(session_id);
                record.resolved_provider = Some(target.provider);
                record.resolved_model = target.model;
                record.reason = Some("user-override".to_owned());
                crate::eval::append_decision_log(&crate::eval::default_log_path(), &record);
                Ok(ResponsePayload::Ack)
            }
            Command::RecordRouteClass {
                session_id,
                class,
                target,
                provider_map,
            } => {
                let mut record = crate::eval::EvalDecisionRecord::empty("route-class");
                record.session_id = Some(session_id);
                record.class = Some(class.id().to_owned());
                record.resolved_provider = Some(target.provider);
                record.resolved_model = target.model;
                record.resolved_effort = target.effort;
                record.reason = Some(
                    if provider_map {
                        "provider-class-map"
                    } else {
                        "global-class-map"
                    }
                    .to_owned(),
                );
                crate::eval::append_decision_log(&crate::eval::default_log_path(), &record);
                Ok(ResponsePayload::Ack)
            }
            Command::LoadEvalUsage => Ok(ResponsePayload::EvalUsage {
                stats: crate::eval::usage_stats(&crate::eval::default_log_path()),
            }),
            Command::LoadUsageHistory {
                window,
                project_roots,
            } => {
                let rates = crate::usage_history::load_rate_table(&self.usage_rates_dir);
                let history = crate::usage_history::scan(
                    &mut self.usage_scan_cache.lock(),
                    &rates,
                    window,
                    &project_roots,
                );
                Ok(ResponsePayload::UsageHistory { history })
            }
            Command::LoadSkills { projects } => {
                let locations = crate::skills::skill_locations(&projects);
                Ok(ResponsePayload::SkillsCatalog {
                    catalog: crate::skills::scan_skills(&locations),
                })
            }
            Command::SetSkillsEnabled { dirs, enabled } => {
                for dir in dirs {
                    crate::skills::set_skill_enabled(&dir, enabled)
                        .map_err(|error| anyhow!(error))?;
                }
                Ok(ResponsePayload::Ack)
            }
            Command::TrashSkills { dirs } => {
                crate::skills::trash_skills(&dirs).map_err(|error| anyhow!(error))?;
                Ok(ResponsePayload::Ack)
            }
            Command::LoadTaskState => {
                self.purge_expired_archived_sessions();
                self.start_archive_detail_prune();
                let boss = self.boss.document();
                // Boss-managed rows ride the catalog even unstarted — a
                // queued employee's task shell is what its roster click
                // selects. Other unstarted rows are client drafts:
                // cataloguing one would project a phantom "New task"
                // skeleton to every client.
                let managed = |session_id: Uuid| {
                    boss.session_id == Some(session_id)
                        || boss
                            .employees
                            .iter()
                            .any(|employee| employee.session_id == session_id)
                        || boss
                            .planning
                            .iter()
                            .any(|plan| plan.session_id == session_id)
                };
                let state = self.task_state.lock();
                Ok(ResponsePayload::TaskState {
                    projects: state
                        .projects
                        .iter()
                        .filter(|project| project.id != boss.identity.id)
                        .cloned()
                        .collect(),
                    sessions: state
                        .sessions
                        .iter()
                        .filter(|session| session.has_started() || managed(session.id))
                        .map(AgentSession::list_projection)
                        .collect(),
                    default_cwd: self.default_cwd.clone(),
                    projectless_root: crate::projectless::workspace_root(),
                })
            }
            Command::SaveTaskState {
                projects,
                live_session_ids: _,
                sessions,
                session_tails,
            } => {
                let active_runtimes = self
                    .sessions
                    .lock()
                    .iter()
                    .map(|(session_id, entry)| (*session_id, entry.runtime_id))
                    .collect::<HashMap<_, _>>();
                // Incremental saves carry only each session's appended tail;
                // the prefix comes from the resident session when it is
                // loaded and from the store's own read connection when it is
                // not — either way outside the merge lock.
                let mut tails: HashMap<Uuid, SessionDetailTail> = session_tails
                    .into_iter()
                    .map(|tail| (tail.session_id, tail))
                    .collect();
                let mut bases: HashMap<Uuid, AgentSession> = HashMap::new();
                if !tails.is_empty() {
                    let cold = {
                        let state = self.task_state.lock();
                        sessions
                            .iter()
                            .filter(|session| tails.contains_key(&session.id))
                            .filter(|session| {
                                !state.sessions.iter().any(|resident| {
                                    resident.id == session.id && resident.detail_loaded
                                })
                            })
                            .map(|session| session.id)
                            .collect::<Vec<_>>()
                    };
                    for session_id in cold {
                        if let Some(stored) = self.task_store.load_session_detail(session_id)? {
                            bases.insert(session_id, stored);
                        }
                    }
                }
                let mut state = self.task_state.lock();
                let removed_project_ids = self.removed_project_ids.lock();
                for mut project in projects {
                    if removed_project_ids.contains(&project.id) {
                        continue;
                    }
                    if let Some(existing) = state
                        .projects
                        .iter_mut()
                        .find(|existing| existing.id == project.id)
                    {
                        preserve_daemon_project_fields(existing, &mut project);
                        *existing = project;
                    } else {
                        state.projects.push(project);
                    }
                }
                drop(removed_project_ids);
                let removed_session_ids = self.removed_session_ids.lock();
                let sessions = sessions
                    .into_iter()
                    .filter(|session| !removed_session_ids.contains(&session.id))
                    .collect::<Vec<_>>();
                drop(removed_session_ids);
                let mut saved_ids = Vec::with_capacity(sessions.len());
                for mut session in sessions {
                    let session_id = session.id;
                    if let Some(tail) = tails.remove(&session_id) {
                        splice_session_tail(&state.sessions, &mut bases, &mut session, tail);
                    }
                    let applied = if let Some(existing) = state
                        .sessions
                        .iter_mut()
                        .find(|existing| existing.id == session_id)
                    {
                        if self.boss.is_managed(session_id) {
                            merge_stale_session_metadata(existing, session);
                            true
                        } else if !session.detail_loaded {
                            merge_session_list_columns(
                                existing,
                                session,
                                active_runtimes.contains_key(&session_id),
                            )
                        } else if existing.has_started() && !session.has_started() {
                            // A session that has started can never become a
                            // draft again; an "empty" loaded projection is a
                            // skeleton that lost its marker, not a cleared
                            // transcript.
                            false
                        } else if session_projection_precedes(
                            existing,
                            &session,
                            active_runtimes.get(&session_id).copied(),
                        ) {
                            merge_stale_session_metadata(existing, session);
                            true
                        } else {
                            preserve_daemon_checkpoints(existing, &mut session);
                            preserve_daemon_queued_messages(existing, &mut session);
                            honor_details_pruned(existing, &mut session);
                            *existing = session;
                            true
                        }
                    } else if session.detail_loaded && session.has_started() {
                        state.sessions.push(session);
                        true
                    } else {
                        // A skeleton can update a known row but never create
                        // one — none of its detail is real. An unstarted draft
                        // owns no row either: cataloguing it would project it
                        // back to every client as a phantom "New task" skeleton.
                        false
                    };
                    if applied {
                        saved_ids.push(session_id);
                    }
                }
                let used_project_ids = state
                    .sessions
                    .iter()
                    .map(|session| session.project_id)
                    .collect::<std::collections::HashSet<_>>();
                let now = crate::model::unix_time();
                state.projects.retain(|project| {
                    !project.is_projectless()
                        || used_project_ids.contains(&project.id)
                        || now.saturating_sub(project.created_at) < UNUSED_PROJECTLESS_GRACE_SECONDS
                });
                for session_id in &saved_ids {
                    state.mark_session_dirty(*session_id);
                }
                // Archiving a task deletes its side chats — the same rule the
                // native app applies. A client that only flips `archived_at`
                // must not leave them in the catalog.
                let mut cascaded = Vec::new();
                let mut queue: Vec<Uuid> = state
                    .sessions
                    .iter()
                    .filter(|session| session.archived_at.is_some())
                    .map(|session| session.id)
                    .collect();
                while let Some(parent) = queue.pop() {
                    for session in state
                        .sessions
                        .iter()
                        .filter(|session| session.side_chat_of == Some(parent))
                    {
                        cascaded.push(session.id);
                        queue.push(session.id);
                    }
                }
                let cascaded_roots = state
                    .sessions
                    .iter()
                    .filter(|session| cascaded.contains(&session.id))
                    .filter_map(|session| session.workspace.path().map(Path::to_path_buf))
                    .collect::<Vec<_>>();
                if !cascaded.is_empty() {
                    state
                        .sessions
                        .retain(|session| !cascaded.contains(&session.id));
                    let mut removed = self.removed_session_ids.lock();
                    for id in &cascaded {
                        removed.insert(*id);
                    }
                }
                // The batch claims the dirty marks up front so a session
                // re-dirtied while the write runs unlocked keeps its flag.
                let batch = self.task_store.save_batch(&mut state);
                state.unmark_sessions_dirty(&batch.dirty_ids);
                drop(state);
                if let Err(error) = self.task_store.write_batch(&batch) {
                    // The rows never landed — the marks go back so the next
                    // save retries them.
                    let mut state = self.task_state.lock();
                    for id in &batch.dirty_ids {
                        state.mark_session_dirty(*id);
                    }
                    return Err(error.into());
                }
                let removed_terminals = self.sweep_orphaned_terminals(&cascaded, &cascaded_roots);
                drop_detached(removed_terminals);
                let removed_runtimes = cascaded
                    .iter()
                    .filter_map(|id| self.sessions.lock().remove(id))
                    .collect::<Vec<_>>();
                for runtime in &removed_runtimes {
                    runtime.driver.begin_shutdown();
                }
                drop_detached(removed_runtimes);
                for id in cascaded {
                    self.agent.clear_session(id);
                }
                let mut state = self.task_state.lock();
                let sessions = saved_ids
                    .into_iter()
                    .filter_map(|session_id| {
                        state
                            .sessions
                            .iter()
                            .find(|session| session.id == session_id)
                            .cloned()
                    })
                    .collect::<Vec<_>>();
                self.auto_prompts.consider(
                    &sessions
                        .iter()
                        .filter(|session| !self.boss.is_managed(session.id))
                        .cloned()
                        .collect::<Vec<_>>(),
                );
                // The save above can adopt full transcripts for every session
                // the client touched. Keep only the recent window resident;
                // the echoed clones above still carry the saved detail.
                trim_resident_transcripts(&mut state, &active_runtimes.keys().copied().collect());
                Ok(ResponsePayload::TaskStateSaved { sessions })
            }
            Command::RemoveSession => {
                self.remove_session(session_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::RemoveProject { project_id } => {
                self.remove_project(project_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::HydrateSession { session_id } => {
                // The skeleton leaves the global lock for the SQLite read and
                // JSON parse — `load_session_detail` opens its own connection,
                // so it cannot queue behind a save holding `task_state` or
                // `storage` for its whole write. The lock is retaken only to
                // merge the detail back and trim the resident window.
                let mut session = {
                    let state = self.task_state.lock();
                    let Some(session) = state
                        .sessions
                        .iter()
                        .find(|session| session.id == session_id)
                        .cloned()
                    else {
                        return Ok(ResponsePayload::Session { session: None });
                    };
                    session
                };
                if !session.detail_loaded {
                    match self.task_store.load_session_detail(session_id)? {
                        Some(detail) => {
                            crate::persistence::apply_session_detail(&mut session, detail);
                        }
                        // A session with no stored row is already whole.
                        None => session.detail_loaded = true,
                    }
                }
                // Live runtimes stay resident; everything else is trimmed to
                // the recency window once the response is built.
                let pinned = self.sessions.lock().keys().copied().collect();
                let mut state = self.task_state.lock();
                let session = match state
                    .sessions
                    .iter_mut()
                    .find(|existing| existing.id == session_id)
                {
                    Some(existing) => {
                        // A racing hydration may have landed meanwhile;
                        // applying the same stored detail again is harmless,
                        // but overwriting a session that gained newer unsaved
                        // detail is not.
                        if !existing.detail_loaded && session.detail_loaded {
                            crate::persistence::apply_session_detail(existing, session);
                        }
                        Some(existing.clone())
                    }
                    // Removed while the read ran — still answer with what was
                    // stored; the client drops sessions it no longer has.
                    None => Some(session),
                };
                trim_resident_transcripts(&mut state, &pinned);
                Ok(ResponsePayload::Session { session })
            }
            Command::IndexSession => Ok(ResponsePayload::Ack),
            Command::SearchSessionMessages {
                query,
                limit,
                scope,
            } => {
                let matches = self.search_session_messages(&query, limit, scope, None, None)?;
                Ok(ResponsePayload::SessionMessageMatches { matches })
            }
            Command::ListProviderSessions { provider, limit } => {
                const MAX_PROVIDER_SESSIONS: usize = 500;
                let limit = limit.min(MAX_PROVIDER_SESSIONS);
                if limit == 0 {
                    return Ok(ResponsePayload::ProviderSessions {
                        sessions: Vec::new(),
                        status: Default::default(),
                    });
                }
                ensure_shell_environment();
                let settings = self.settings.get();
                if settings.disabled_providers.contains(&provider) {
                    return Ok(ResponsePayload::ProviderSessions {
                        sessions: Vec::new(),
                        status: Default::default(),
                    });
                }
                let binary_override = settings
                    .provider_binary_overrides
                    .get(&provider)
                    .map(String::as_str);
                let Some(binary) = crate::model::provider_probe(provider, binary_override).path
                else {
                    return Ok(ResponsePayload::ProviderSessions {
                        sessions: Vec::new(),
                        status: ProviderSessionCatalogStatus::BinaryMissing,
                    });
                };
                // Discovery is deliberately provider-scoped. Opening Resume
                // must not start every installed agent CLI, and another
                // provider is queried only after the user explicitly picks it.
                let mut catalog: crate::acp_session::ProviderSessionCatalog = match provider {
                    // Antigravity conversations live in its own TUI; there is
                    // no Goddard transcript to import.
                    ProviderKind::Antigravity => {
                        crate::acp_session::ProviderSessionCatalog::unsupported()
                    }
                    ProviderKind::Amp => {
                        crate::amp_session::list_provider_sessions(&binary, limit)?.into()
                    }
                    ProviderKind::Claude => {
                        crate::claude_session::list_provider_sessions(limit)?.into()
                    }
                    ProviderKind::Codex => {
                        crate::codex_session::list_provider_sessions(&binary, limit)?.into()
                    }
                    ProviderKind::Copilot => {
                        crate::copilot_session::list_provider_sessions(limit)?.into()
                    }
                    ProviderKind::Cursor
                    | ProviderKind::Devin
                    | ProviderKind::Fx
                    | ProviderKind::Droid
                    | ProviderKind::Goose => {
                        crate::acp_session::list_provider_sessions(provider, &binary, &[], limit)?
                    }
                    ProviderKind::OpenCode => {
                        crate::opencode_session::list_provider_sessions(&binary, limit)?.into()
                    }

                    ProviderKind::DeepSeek => {
                        crate::deepseek_session::list_provider_sessions(&binary, limit)?.into()
                    }
                    ProviderKind::Grok => {
                        crate::grok_session::list_provider_sessions(limit)?.into()
                    }
                    ProviderKind::Kimi => {
                        crate::kimi_session::list_provider_sessions(limit)?.into()
                    }
                    ProviderKind::Muse => {
                        crate::muse_session::list_provider_sessions(&binary, limit)?.into()
                    }
                    ProviderKind::OhMyPi | ProviderKind::Pi => {
                        crate::pi_session::list_provider_sessions(provider, limit)?.into()
                    }
                };
                catalog.sessions.sort_by(|a, b| {
                    b.updated_at
                        .cmp(&a.updated_at)
                        .then_with(|| a.title.cmp(&b.title))
                });
                let imported = {
                    let state = self.task_state.lock();
                    state
                        .sessions
                        .iter()
                        .filter_map(|session| session.provider_cursor.as_ref())
                        .map(|cursor| (cursor.provider(), cursor.native_id().to_owned()))
                        .collect::<HashSet<_>>()
                };
                catalog.sessions.retain(|session| {
                    !imported.contains(&(session.provider(), session.cursor.native_id().to_owned()))
                });
                catalog.sessions.truncate(limit);
                for session in &mut catalog.sessions {
                    session.cwd_missing = !session.cwd.is_dir();
                }
                Ok(ResponsePayload::ProviderSessions {
                    sessions: catalog.sessions,
                    status: catalog.status,
                })
            }
            Command::LoadProviderSession {
                cursor,
                cwd,
                updated_at,
            } => {
                // Preserve every native turn shell for exact provider turn
                // numbering, but bound imported display text to recent turns.
                const VISIBLE_TURN_LIMIT: usize = 100;
                // A `cwd_missing` session's recorded folder is gone; launch
                // and load in the nearest surviving ancestor instead.
                let cwd = crate::acp_session::resume_working_directory(&cwd);
                let history = match &cursor {
                    ProviderResumeCursor::Amp { thread_id, .. } => {
                        let binary = self.provider_binary(ProviderKind::Amp)?;
                        crate::amp_session::provider_session_history(
                            &binary,
                            &cwd,
                            thread_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Claude { session_id, .. } => {
                        self.provider_binary(ProviderKind::Claude)?;
                        crate::claude_session::provider_session_history(
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Codex { thread_id } => {
                        let binary = self.provider_binary(ProviderKind::Codex)?;
                        crate::codex_session::provider_session_history(
                            &binary,
                            thread_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Copilot { session_id } => {
                        crate::copilot_session::provider_session_history(
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    // OpenCode is not an ACP provider: its history comes from
                    // the adopted background service's own export route.
                    ProviderResumeCursor::OpenCode { session_id, .. } => {
                        let binary = self.provider_binary(ProviderKind::OpenCode)?;
                        crate::opencode_session::provider_session_history(
                            &binary,
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Cursor { session_id, .. }
                    | ProviderResumeCursor::Devin { session_id }
                    | ProviderResumeCursor::Fx { session_id }
                    | ProviderResumeCursor::Goose { session_id }
                    | ProviderResumeCursor::Grok { session_id }
                    | ProviderResumeCursor::Kimi { session_id }
                    | ProviderResumeCursor::Droid { session_id } => {
                        let provider = cursor.provider();
                        let binary = self.provider_binary(provider)?;
                        crate::acp_session::provider_session_history(
                            provider,
                            &binary,
                            &cwd,
                            session_id,
                            VISIBLE_TURN_LIMIT,
                            updated_at,
                        )?
                    }
                    ProviderResumeCursor::Muse { session_id, .. } => {
                        let binary = self.provider_binary(ProviderKind::Muse)?;
                        crate::muse_session::provider_session_history(
                            &binary,
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::DeepSeek { session_id } => {
                        let binary = self.provider_binary(ProviderKind::DeepSeek)?;
                        crate::deepseek_session::provider_session_history(
                            &binary,
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::OhMyPi {
                        session_id,
                        session_file,
                    }
                    | ProviderResumeCursor::Pi {
                        session_id,
                        session_file,
                    } => {
                        self.provider_binary(cursor.provider())?;
                        let session_file = session_file.as_deref().ok_or_else(|| {
                            anyhow!(
                                "{} did not report its native session file",
                                cursor.provider().display_name()
                            )
                        })?;
                        crate::pi_session::provider_session_history(
                            cursor.provider(),
                            session_id,
                            session_file,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Antigravity { .. } => {
                        bail!(
                            "Antigravity conversations live in its own TUI; there is no transcript to import"
                        )
                    }
                };
                Ok(ResponsePayload::ProviderSessionHistory {
                    history,
                    resolved_cwd: Some(cwd),
                })
            }
            Command::LoadComposerDrafts => {
                if !self.settings.get().composer_drafts_experiment_enabled {
                    anyhow::bail!("composer drafts experiment is disabled");
                }
                Ok(ResponsePayload::ComposerDrafts {
                    drafts: self.composer_drafts.load()?,
                })
            }
            Command::SaveComposerDrafts { drafts, generation } => {
                if !self.settings.get().composer_drafts_experiment_enabled {
                    anyhow::bail!("composer drafts experiment is disabled");
                }
                self.composer_drafts.save(drafts, generation)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ApplyComposerDraftChanges { changes } => {
                if !self.settings.get().composer_drafts_experiment_enabled {
                    anyhow::bail!("composer drafts experiment is disabled");
                }
                self.composer_drafts.apply_changes(changes)?;
                Ok(ResponsePayload::Ack)
            }
            Command::StoreBlob { mime_type, bytes } => {
                let reference = self
                    .task_store
                    .blobs()
                    .store_image_bytes(&mime_type, &bytes)?;
                let path = self
                    .task_store
                    .blobs()
                    .path_for(&reference)
                    .ok_or_else(|| anyhow!("stored blob has no daemon path"))?;
                Ok(ResponsePayload::BlobStored { reference, path })
            }
            Command::ImportAttachment { name, upload } => Ok(ResponsePayload::AttachmentStored {
                attachment: self.attachments.import(&name, upload)?,
            }),
            Command::ImportPathAttachment { path } => Ok(ResponsePayload::AttachmentStored {
                attachment: self.attachments.import_path(&path)?,
            }),
            Command::ReadBlob { reference } => {
                let path = self
                    .task_store
                    .blobs()
                    .path_for(&reference)
                    .ok_or_else(|| anyhow!("invalid blob reference"))?;
                Ok(ResponsePayload::BlobData {
                    bytes: std::fs::read(path)?,
                })
            }
            Command::ReadAttachment { reference, path } => Ok(ResponsePayload::BlobData {
                bytes: self.attachments.read_file(&reference, &path)?,
            }),
            Command::SweepBlobs => {
                self.task_store.blob_sweep()();
                Ok(ResponsePayload::Ack)
            }
            Command::ForkSessionFromResponse { turn_count } => {
                let (session, checkpoint_warning) =
                    self.fork_session_from_response(session_id, turn_count)?;
                Ok(ResponsePayload::SessionForked {
                    session,
                    checkpoint_warning,
                })
            }
            Command::RewindSessionToMessage { turn_count } => {
                let (session, cleanup_warning) =
                    self.rewind_session_to_message(session_id, turn_count)?;
                Ok(ResponsePayload::SessionRewound {
                    session,
                    cleanup_warning,
                })
            }
            Command::ForkProviderSession { request } => {
                Ok(ResponsePayload::ProviderSessionForked {
                    result: fork_provider_session(request)?,
                })
            }
            Command::Workspace {
                operation:
                    WorkspaceOperation::CaptureTurn {
                        cwd,
                        session_id,
                        turn_count,
                        untouched,
                    },
            } => Ok(ResponsePayload::Workspace {
                result: WorkspaceResult::Checkpoint {
                    checkpoint: self
                        .capture_turn_checkpoint(cwd, session_id, turn_count, untouched)?,
                },
            }),
            Command::Workspace { operation } => {
                // Commit/push/land move local refs — prompt the share
                // poll so synced branches push and notify promptly
                // rather than waiting out the interval.
                let kick = matches!(
                    operation,
                    WorkspaceOperation::Commit { .. }
                        | WorkspaceOperation::Push { .. }
                        | WorkspaceOperation::PushBase { .. }
                        | WorkspaceOperation::Land { .. }
                        | WorkspaceOperation::RebaseOnto { .. }
                );
                let review_move = match &operation {
                    WorkspaceOperation::ReviewApprove { cwd, .. } => {
                        Some((cwd.clone(), ReviewMove::Approved))
                    }
                    WorkspaceOperation::ReviewReject { cwd, .. } => {
                        Some((cwd.clone(), ReviewMove::Rejected))
                    }
                    WorkspaceOperation::ReviewPromote { cwd } => {
                        Some((cwd.clone(), ReviewMove::Promoted))
                    }
                    _ => None,
                };
                // Only the Review* operations read the QA branch — resolve
                // it against the repository `cwd` belongs to so a project
                // override retargets its whole review train.
                let qa_branch = match &operation {
                    WorkspaceOperation::ReviewQueue { cwd }
                    | WorkspaceOperation::ReviewApprove { cwd, .. }
                    | WorkspaceOperation::ReviewReject { cwd, .. }
                    | WorkspaceOperation::ReviewPromote { cwd } => self.project_qa_branch(cwd),
                    _ => self.settings.get().qa_branch,
                };
                let result = crate::workspace::execute(operation, &qa_branch)?;
                if kick {
                    self.share.note_repo_activity();
                }
                if let Some((cwd, review_move)) = review_move {
                    self.notify_review_moved(&cwd, review_move, &result);
                }
                Ok(ResponsePayload::Workspace { result })
            }
            Command::OpenTerminal {
                cwd,
                cols,
                rows,
                owner,
            } => {
                // Claim before spawning so the shell's startup output can
                // never broadcast to subscribers that didn't open it.
                events.claim_terminal_channel(Vec::new());
                match self.open_terminal(&cwd, cols, rows, events.clone()) {
                    Ok(terminal) => {
                        let previous = self.terminals.lock().insert(
                            session_id,
                            TerminalEntry {
                                runtime_id,
                                owner,
                                cwd,
                                terminal,
                            },
                        );
                        drop_detached(previous);
                        Ok(ResponsePayload::Ack)
                    }
                    Err(error) => {
                        events.release_terminal_channel(session_id);
                        Err(error)
                    }
                }
            }
            Command::WriteTerminal { data } => {
                let terminals = self.terminals.lock();
                let entry = terminals
                    .get(&session_id)
                    .ok_or_else(|| anyhow!("daemon terminal {session_id} is not running"))?;
                if entry.runtime_id != runtime_id {
                    bail!(
                        "daemon terminal {session_id} belongs to runtime {}, not {runtime_id}",
                        entry.runtime_id
                    );
                }
                if events.terminal_channel_owner(session_id) != Some(events.source_subscriber_id())
                {
                    events.claim_terminal_channel(entry.terminal.take_tail());
                }
                entry.terminal.write(data)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ResizeTerminal { cols, rows } => {
                let terminals = self.terminals.lock();
                let entry = terminals
                    .get(&session_id)
                    .ok_or_else(|| anyhow!("daemon terminal {session_id} is not running"))?;
                if entry.runtime_id != runtime_id {
                    bail!(
                        "daemon terminal {session_id} belongs to runtime {}, not {runtime_id}",
                        entry.runtime_id
                    );
                }
                if events.terminal_channel_owner(session_id) != Some(events.source_subscriber_id())
                {
                    events.claim_terminal_channel(entry.terminal.take_tail());
                }
                entry.terminal.resize(cols, rows);
                Ok(ResponsePayload::Ack)
            }
            Command::CloseTerminal => {
                let removed = {
                    let mut terminals = self.terminals.lock();
                    if let Some(entry) = terminals.get(&session_id)
                        && entry.runtime_id != runtime_id
                    {
                        bail!(
                            "daemon terminal {session_id} belongs to runtime {}, not {runtime_id}",
                            entry.runtime_id
                        );
                    }
                    terminals.remove(&session_id)
                };
                events.release_terminal_channel(session_id);
                drop_detached(removed.map(|entry| entry.terminal));
                Ok(ResponsePayload::Ack)
            }
            Command::Start { options } => {
                let previous = self.sessions.lock().remove(&session_id);
                if let Some(previous) = &previous {
                    previous.driver.begin_shutdown();
                }
                drop_detached(previous);
                // The replaced runtime's scoped credential dies with it; the
                // new runtime mints its own inside `spawn_runtime`.
                self.agent.revoke_session(session_id);
                let provider = decode_enum(&options.provider)?;
                let options = DriverStartOptions {
                    binary: options.binary,
                    cwd: options.cwd,
                    mode: decode_enum(&options.mode)?,
                    model: options.model,
                    reasoning_effort: options.reasoning_effort,
                    service_tier: options.service_tier,
                    context_window: options.context_window,
                    agent_preset: options.agent_preset,
                    computer_use_enabled: options.computer_use_enabled,
                    agent: None,
                    read_own_transcript: options.read_own_transcript,
                    subagents: None,
                    // Filled in by `spawn_runtime` — the daemon owns the
                    // catalog, never the wire.
                    computer_use_runtime: None,
                    mcp_servers: Vec::new(),
                    http_mcp_capability_recorder: None,
                    provider_cursor: options
                        .provider_cursor
                        .map(serde_json::from_value)
                        .transpose()
                        .context("daemon received an invalid provider cursor")?,
                    eval: None,
                    sandbox: None,
                    allow_model_fallback: false,
                    ephemeral: false,
                };
                let resumable = options.provider_cursor.is_some();
                let cwd = options.cwd.clone();
                let (handle, computer_use_available) =
                    self.spawn_runtime(session_id, runtime_id, provider, options, events)?;
                let supports_steer = handle.supports_steer();
                let supports_user_input_actions = handle.supports_user_input_actions();
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
                Ok(ResponsePayload::Started {
                    supports_steer,
                    supports_user_input_actions,
                })
            }
            Command::CloseSession => {
                let removed = {
                    let mut sessions = self.sessions.lock();
                    sessions
                        .get(&session_id)
                        // A nil runtime id is an explicit task-wide eviction
                        // from clients that no longer hold an attachment.
                        .is_some_and(|entry| runtime_id.is_nil() || entry.runtime_id == runtime_id)
                        .then(|| sessions.remove(&session_id))
                        .flatten()
                };
                if let Some(removed) = &removed {
                    removed.driver.begin_shutdown();
                }
                drop_detached(removed);
                self.agent.revoke_session(session_id);
                Ok(ResponsePayload::Ack)
            }
            Command::AgentCreateSession {
                provider,
                model,
                project,
                workspace,
                base_branch,
                prompt,
                title,
                reasoning_effort,
                service_tier,
                context_window,
            } => {
                // A scoped credential names its owning session; a master-token
                // request may attribute the prompt to `request.session_id`
                // when it is a known task.
                let sender = agent.or_else(|| {
                    (!session_id.is_nil() && self.known_session(session_id)).then_some(session_id)
                });
                self.agent_create_session(
                    sender,
                    AgentCreateSelection {
                        provider,
                        model,
                        title,
                        reasoning_effort,
                        service_tier,
                        context_window,
                    },
                    project,
                    workspace,
                    base_branch,
                    prompt,
                    events,
                )
            }
            Command::AgentPrompt {
                task_id,
                thread_id,
                provider,
                prompt,
                delivery,
            } => {
                let sender = agent.or_else(|| {
                    (!session_id.is_nil() && self.known_session(session_id)).then_some(session_id)
                });
                self.agent_prompt(
                    sender, task_id, thread_id, provider, prompt, delivery, events,
                )
            }
            Command::AgentRenameSelf { title } => self.agent_rename_self(agent, &title, &events),
            Command::AgentProposeArchive { task_ids, reason } => {
                self.agent_propose_archive(agent, task_ids, reason, &events)
            }
            Command::AgentMergeSubmit => self.agent_merge_submit(agent),
            Command::AgentReadSession {
                task_id,
                thread_id,
                provider,
                turn,
            } => self.agent_read_session(agent, task_id, thread_id, provider, turn),
            Command::AgentSearchSessions { query, last_turns } => {
                self.agent_search_sessions(agent, session_id, &query, last_turns)
            }
            Command::AgentHistorySearch {
                query,
                project,
                person,
                after,
                before,
                kind,
                limit,
                offset,
            } => self.agent_history_search(
                agent,
                session_id,
                &query,
                project.as_deref(),
                person.as_deref(),
                after.as_deref(),
                before.as_deref(),
                kind,
                limit,
                offset,
            ),
            Command::AgentProjectMap {
                query,
                path,
                max_tokens,
                intent,
                anchors,
                known_paths,
            } => self.agent_project_map(
                agent,
                &query,
                path.as_deref(),
                max_tokens,
                intent,
                &anchors,
                &known_paths,
            ),
            Command::AgentAsk { questions } => {
                self.agent_ask(session_id, agent, questions, &events)
            }
            Command::AgentComputerUse {
                code,
                timeout_ms,
                title,
            } => self.agent_computer_use(
                session_id,
                agent,
                Some(&code),
                timeout_ms,
                title.as_deref(),
            ),
            Command::AgentComputerUseRun { request } => {
                self.agent_computer_use_run(session_id, agent, request)
            }
            Command::AgentComputerUseReset => {
                self.agent_computer_use(session_id, agent, None, None, None)
            }
            Command::AgentResources { operation } => {
                let owner =
                    agent.context("resource reservations require a scoped task credential")?;
                // Admission tickets are daemon-owned — model slots and the
                // dispatch order they encode belong to the summon queue,
                // not to task-scoped callers.
                if matches!(
                    operation,
                    waku_protocol::resources::ResourceOperation::Admission { .. }
                ) {
                    bail!("admission reservations are daemon-internal");
                }
                // An acquire can park its caller in a wait for minutes; the
                // human-facing boss delegates waits to employees instead.
                if self.boss.is_boss_principal(owner)
                    && matches!(
                        operation,
                        waku_protocol::resources::ResourceOperation::Acquire { .. }
                    )
                {
                    bail!(
                        "the boss stays available to the human — summon an employee to run workloads that reserve host resources"
                    );
                }
                let status = self.resource_broker()?.operate(owner, operation)?;
                if let Some(id) = status.request_id {
                    self.agent.note_resource(owner, id);
                    if let Some(reservation) = status.reservations.iter().find(|r| r.id == id) {
                        let waiting = reservation.granted_at.is_none() && !reservation.cancelled;
                        let title = if waiting {
                            crate::resource_broker::waiting_title(reservation, &status)
                        } else {
                            format!("Resources: {}", reservation.purpose)
                        };
                        if let Ok(event) = event_to_wire(DriverEvent::Activity {
                            id: Some(format!("resource-{id}")),
                            kind: crate::model::ActivityKind::Tool,
                            title,
                            detail: None,
                            complete: !waiting,
                        }) {
                            let _ = events.send(event);
                        }
                    } else if let Ok(event) = event_to_wire(DriverEvent::Activity {
                        id: Some(format!("resource-{id}")),
                        kind: crate::model::ActivityKind::Tool,
                        title: "Resource reservation ended".into(),
                        detail: None,
                        complete: true,
                    }) {
                        let _ = events.send(event);
                    }
                }
                Ok(ResponsePayload::AgentResources { status })
            }
            Command::AgentListModels => self.agent_model_options(),
            Command::CancelQueuedPrompt { queued_message_id } => {
                self.cancel_queued_prompt(session_id, queued_message_id, &events)
            }
            command => {
                if matches!(command, Command::Cancel) {
                    self.agent.cancel_resources(session_id);
                }
                // Daemon-owned `agentAsk`/`agentRenameSelf` requests resolve
                // here — their request ids never reached the provider, so
                // the driver has nothing parked under them. Every attached
                // client hears the settle so cards answered elsewhere drop.
                let settled = if self.agent.resolve_user_input(session_id, &command) {
                    match &command {
                        Command::RespondUserInput { request_id, .. }
                        | Command::ClarifyUserInput { request_id, .. }
                        | Command::CancelUserInput { request_id } => Some(request_id.clone()),
                        _ => None,
                    }
                } else {
                    self.agent.resolve_permission(session_id, &command)
                };
                if let Some(request_id) = settled {
                    if let Ok(wire) = event_to_wire(DriverEvent::RequestSettled { request_id }) {
                        let _ = events.send(wire);
                    }
                    return Ok(ResponsePayload::Ack);
                }
                // Quarantined transfer sessions still take interactive
                // prompts — the sandbox is the boundary, and the quarantine
                // flag only keeps unattended senders (agent prompts,
                // automations) out until the user trusts the transfer.
                let driver = {
                    let mut sessions = self.sessions.lock();
                    let entry = sessions
                        .get_mut(&session_id)
                        .ok_or_else(|| anyhow!("daemon session {session_id} is not running"))?;
                    if entry.runtime_id != runtime_id {
                        bail!(
                            "daemon session {session_id} belongs to runtime {}, not {runtime_id}",
                            entry.runtime_id
                        );
                    }
                    // Serving a request is activity — the idle reaper must
                    // not reclaim a runtime a client just talked to.
                    entry.last_active = std::time::Instant::now();
                    entry.driver.clone()
                };
                if let Command::Prompt {
                    prompt,
                    turn_id,
                    message_id,
                    hidden,
                    ..
                } = &command
                {
                    // Publish the submission into the runtime's event stream
                    // before the provider can start the turn. Every attached
                    // client mirrors the user message and its turn from this
                    // event, so the submitting client's own save is no longer
                    // the only record of the prompt — a follower that only
                    // knew the provider's `turnStarted` used to persist a
                    // projection without it, erasing the message for everyone.
                    self.boss.require_active(session_id)?;
                    let submitted_message_id = message_id.unwrap_or_else(Uuid::new_v4);
                    let submitted = DriverEvent::PromptSubmitted {
                        message: prompt.clone(),
                        turn_id: turn_id.unwrap_or_else(Uuid::new_v4),
                        message_id: submitted_message_id,
                        sent_by_task: None,
                        hidden: *hidden,
                        report_trigger: None,
                        reference_context: None,
                    };
                    if self.boss.is_managed(session_id) {
                        record_boss_event(
                            &self.task_state,
                            &self.task_store,
                            session_id,
                            &submitted,
                        )?;
                    }
                    events.send(event_to_wire(submitted)?)?;
                    if !*hidden && self.boss.is_boss_principal(session_id) {
                        // A boss op reaching back to the user — a `terminal`
                        // intent — is owed to the client this prompt came
                        // from. Record it while the prompting connection's
                        // subscriber id is still on the request's sink; the
                        // agent sentinel only ever marks connections that
                        // cannot render a terminal anyway.
                        let source = events.source_subscriber_id();
                        if source != u64::MAX {
                            self.boss_prompt_subscribers
                                .lock()
                                .insert(session_id, source);
                        }
                    }
                    if !*hidden && self.boss.is_boss(session_id) {
                        self.route_boss_prompt(session_id, prompt);
                    }
                }
                let mut command = command;
                if let Command::Prompt { prompt, hidden, .. } = &mut command
                    && !*hidden
                {
                    if driver.supports_steer() {
                        // The prompt reaches the provider exactly as typed —
                        // title generation and first-prompt echoes stay
                        // clean — and the session's context blocks follow as
                        // a hidden steer.
                        let task = prompt.clone();
                        *prompt = self.boss_outbound_prompt(session_id, std::mem::take(prompt));
                        let result = handle_driver_command(&driver, command);
                        self.steer_first_prompt_context(session_id, &task, &driver, &events);
                        return result;
                    }
                    // The agent-surface note, a side chat's parent index,
                    // and project memory ride the first
                    // visible prompt: the wire event above already
                    // published the user's text, so the injected blocks
                    // reach the provider without entering the transcript as
                    // a user message.
                    *prompt =
                        self.prepend_agent_surface(session_id, &driver, std::mem::take(prompt));
                    if let Some(memory) = self.session_memory_block(session_id, &driver) {
                        *prompt = format!("{memory}\n\n{}", std::mem::take(prompt));
                        // The prompt carries it — delivered once sent.
                        self.agent.mark_memory_prepended(session_id);
                    }
                    if let Some(index) = self.side_chat_parent_block(session_id) {
                        *prompt = format!("{index}\n\n{}", std::mem::take(prompt));
                        // The prompt carries it — delivered once sent, no
                        // accept echo to wait on.
                        self.agent.mark_parent_index_prepended(session_id);
                    }
                    if self.boss.is_managed(session_id) {
                        *prompt = self.boss_outbound_prompt(session_id, std::mem::take(prompt));
                    }
                }
                if let Command::Steer {
                    prompt,
                    hidden: true,
                } = &command
                {
                    // A client asked for a hidden injection: record it so the
                    // provider's echo republishes as hidden and no attached
                    // client paints a transcript row for it.
                    self.agent.record_pending_steer(
                        session_id,
                        crate::agent::AgentPrompt {
                            prompt: prompt.clone(),
                            transport: None,
                            sender: None,
                            queued_id: None,
                            context: None,
                            hidden: true,
                            report_trigger: None,
                        },
                    );
                }
                handle_driver_command(&driver, command)
            }
        }
    }

    fn shutdown(&self) {
        let sessions = std::mem::take(&mut *self.sessions.lock());
        drop(sessions);
        self.agent.clear();
        let terminals = std::mem::take(&mut *self.terminals.lock());
        drop(terminals);
        self.share.shutdown();
        self.automations.stop();
    }
}
