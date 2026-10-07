use super::*;

impl WakuBackend {
    pub fn new(settings: DaemonSettingsStore, task_store: StateStore) -> anyhow::Result<Self> {
        let mut task_state = task_store
            .load()
            .context("could not load Goddard task database")?;
        migrate_projectless_state(&task_store, &mut task_state)?;
        let composer_drafts = ComposerDraftStore::for_state_path(task_store.path());
        let attachments = AttachmentStore::new(
            task_store
                .path()
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("attachments"),
        );
        let data_dir = task_store
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_owned();
        let share_dir = data_dir.clone().join("share");
        let settings = Arc::new(settings);
        let integrations =
            crate::integrations::IntegrationService::new(settings.clone(), data_dir.clone())
                .context("could not start the integrations service")?;
        // The proxy port is ephemeral: file providers' managed entries still
        // carry the previous daemon's address until this rewrites them.
        crate::integrations::deliver::sync_file_providers(&settings.get(), &integrations);
        let inference_secrets = crate::integrations::SecretStore::new(data_dir.clone());
        // Migrate documents written before the provider section existed —
        // eval credentials move into the secret store, the provider config
        // fields into `settings.inference`, and the flags rebuild. A no-op
        // for current documents.
        {
            let mut document = settings.get();
            if crate::inference::absorb(&mut document, &inference_secrets) {
                settings.replace(document)?;
            }
        }
        let our_name = std::env::var("USER")
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Goddard".to_owned());
        let usage_rates_dir = data_dir.clone();
        let automations = Arc::new(
            AutomationService::open(data_dir.join("automations.json"))
                .context("could not load Goddard automations")?,
        );
        let boss = Arc::new(if settings.get().boss_experiment_enabled {
            crate::boss::BossService::open(data_dir.join("boss"))?
        } else {
            crate::boss::BossService::disabled(data_dir.join("boss"))
        });
        let auto_prompts = Arc::new(
            AutoPromptService::open(data_dir.join("auto-prompts.json"))
                .context("could not load Goddard auto prompt history")?,
        );
        let task_state = Arc::new(Mutex::new(task_state));
        let task_store = Arc::new(task_store);
        let backend = Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            managed_goal_claims: Mutex::new(HashMap::new()),
            repo_maps: Arc::new((Mutex::new(RepoMaps::default()), Condvar::new())),
            terminals: Arc::new(Mutex::new(HashMap::new())),
            event_source: Mutex::new(EventSink::detached()),
            #[cfg(all(test, unix))]
            terminal_shell: None,
            settings,
            wake: Mutex::new(None),
            integrations,
            inference_secrets,
            task_store,
            task_state,
            task_notifier: Mutex::new(None),
            removed_session_ids: Mutex::new(HashSet::new()),
            removed_project_ids: Mutex::new(HashSet::new()),
            composer_drafts,
            attachments,
            usage_scan_cache: Mutex::new(HashMap::new()),
            codex_reset_credit_lock: Mutex::new(()),
            checkpoint_capture_locks: Mutex::new(HashMap::new()),
            agent: Arc::new(crate::agent::AgentState::default()),
            runtime_start_locks: Mutex::new(HashMap::new()),
            archive_detail_prune_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            idle_reaper_started: std::sync::atomic::AtomicBool::new(false),
            daemon_address: Arc::new(Mutex::new(None)),
            exposed_port: Arc::new(Mutex::new(None)),
            stats: crate::stats::DaemonStats::open(&data_dir),
            usage_rates_dir,
            default_cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            share: Arc::new(crate::share::ShareService::new(
                share_dir.clone(),
                our_name.clone(),
            )),
            pairing: Arc::new(crate::pairing::PairingService::new(
                &data_dir,
                our_name.clone(),
            )),
            data_dir,
            our_name,
            automations,
            boss,
            auto_prompts,
            boss_prompt_subscribers: Mutex::new(HashMap::new()),
            summon_wake: Arc::new((Mutex::new(false), Condvar::new())),
            summon_scheduler_started: std::sync::atomic::AtomicBool::new(false),
            broker_root: Mutex::new(None),
        };
        // Sessions summoned before `boss_managed` existed carry no stamp;
        // the roster still names them, so mark them now — once an employee
        // retires its record is gone and the task would leak back into the
        // ordinary lists.
        if backend.settings.get().boss_experiment_enabled {
            let document = backend.boss.document();
            let managed: HashSet<Uuid> = document
                .session_id
                .into_iter()
                .chain(
                    document
                        .employees
                        .iter()
                        .map(|employee| employee.session_id),
                )
                .collect();
            if !managed.is_empty() {
                let mut state = backend.task_state.lock();
                let stamped: Vec<Uuid> = state
                    .sessions
                    .iter_mut()
                    .filter(|session| !session.boss_managed && managed.contains(&session.id))
                    .map(|session| {
                        session.boss_managed = true;
                        session.id
                    })
                    .collect();
                if !stamped.is_empty() {
                    for id in stamped {
                        state.mark_session_dirty(id);
                    }
                    if let Err(error) = backend.task_store.save(&mut state) {
                        eprintln!("could not stamp boss-managed sessions: {error:#}");
                    }
                }
            }
        }
        backend.purge_expired_archived_sessions();
        backend.start_archive_detail_prune();
        backend.install_link_handlers();
        {
            let task_state = backend.task_state.clone();
            let task_store = backend.task_store.clone();
            let share_dir = share_dir.clone();
            backend
                .share
                .set_transfer_hook(Arc::new(
                    move |transfer, peer_name| match create_transfer_session(
                        &task_state,
                        &task_store,
                        &share_dir,
                        transfer,
                        peer_name,
                    ) {
                        Ok(session_id) => Some(session_id),
                        Err(error) => {
                            eprintln!("could not create transfer session: {error:#}");
                            None
                        }
                    },
                ));
        }
        {
            let task_state = backend.task_state.clone();
            let task_store = backend.task_store.clone();
            let share_dir = share_dir.clone();
            backend.share.set_chat_hook(Arc::new(move |delivery| {
                match create_chat_session(&task_state, &task_store, &share_dir, &delivery) {
                    Ok(session_id) => Some(session_id),
                    Err(error) => {
                        eprintln!("could not create chat session: {error:#}");
                        None
                    }
                }
            }));
        }
        {
            // The share layer's view of local projects — paths, names,
            // and `origin` URLs for share matching. Resolving every
            // remote is a git call per project, so cache briefly.
            let task_state = backend.task_state.clone();
            let cache = Arc::new(Mutex::new(
                None::<(std::time::Instant, Vec<crate::share::RepoInfo>)>,
            ));
            backend.share.set_repo_resolver(Arc::new(move || {
                let mut cache = cache.lock();
                let stale = cache
                    .as_ref()
                    .is_none_or(|(at, _)| at.elapsed() > std::time::Duration::from_secs(60));
                if stale {
                    let projects = task_state.lock().projects.clone();
                    let repos = projects
                        .iter()
                        .map(|project| crate::share::RepoInfo {
                            path: project.path.clone(),
                            name: project.name.clone(),
                            origin_url: crate::git_branch::remote_url(&project.path, "origin")
                                .ok()
                                .flatten(),
                        })
                        .collect();
                    *cache = Some((std::time::Instant::now(), repos));
                }
                cache
                    .as_ref()
                    .map(|(_, repos)| repos.to_vec())
                    .unwrap_or_default()
            }));
        }
        {
            // The share layer's view of sessions — what friends may list
            // and watch on projects shared with session sharing on.
            // Archived sessions and side chats stay hidden, matching
            // task-list semantics.
            let task_state = backend.task_state.clone();
            backend
                .share
                .set_session_source(crate::share::SessionSource {
                    list: Arc::new({
                        let task_state = task_state.clone();
                        move |repo_path| {
                            let state = task_state.lock();
                            let Some(project) = state
                                .projects
                                .iter()
                                .find(|project| project.path == repo_path)
                            else {
                                return Vec::new();
                            };
                            state
                                .sessions
                                .iter()
                                .filter(|session| {
                                    session.project_id == project.id
                                        && session.archived_at.is_none()
                                        && session.side_chat_of.is_none()
                                })
                                .map(|session| waku_protocol::friends::SharedSessionSummary {
                                    session_id: session.id,
                                    title: session.title.clone(),
                                    auto_title: session.auto_title.clone(),
                                    status: session.status,
                                    created_at: session.created_at,
                                    last_reply_at: session.last_reply_at,
                                })
                                .collect()
                        }
                    }),
                    snapshot: Arc::new(move |session_id| {
                        task_state
                            .lock()
                            .sessions
                            .iter()
                            .find(|session| session.id == session_id)
                            .cloned()
                    }),
                });
        }
        backend.apply_wake_setting();
        // Stage daemon-owned copies of the packaged runtime resources while
        // they still exist: a rebuilt or collected target directory can take
        // the executable's neighbors away under a long-running daemon, and
        // every executable-relative resolver then fails at once.
        if let Err(error) = std::thread::Builder::new()
            .name("goddard-runtime-stage".into())
            .spawn(crate::computer_use::stage_runtime_resources)
        {
            eprintln!("goddard-daemon: could not spawn runtime staging thread: {error:#}");
        }
        Ok(backend)
    }

    /// Acquire or release the host sleep assertion to match the stored
    /// `keep_awake` flag. Runs at construction and after every settings
    /// write that can carry the flag.
    pub(super) fn apply_wake_setting(&self) {
        let enabled = self.settings.get().keep_awake;
        let mut wake = self.wake.lock();
        if enabled != wake.is_some() {
            *wake = enabled.then(|| {
                crate::power::SleepAssertion::acquire(
                    "Goddard keeps this host awake so connected devices stay reachable",
                )
            });
        }
    }

    /// Record the daemon's bound address for `GODDARD_DAEMON_ADDRESS`
    /// injection. Called once by the daemon executable before it starts
    /// serving; providers launched while it is unset get no agent surface.
    pub fn set_daemon_address(&self, address: String) {
        *self.daemon_address.lock() = Some(address);
    }

    /// Append the clean-exit marker to `daemon-stats.jsonl` — the next boot
    /// reads its absence as an abnormal death. Called by the daemon
    /// executable after `serve` returns on an orderly shutdown.
    pub fn mark_clean_shutdown(&self) {
        self.stats.mark_clean_shutdown();
    }

    /// Start the automation scheduler: reconcile runs a previous daemon
    /// left open, then tick. Called once by the daemon executable before it
    /// starts serving — the service needs the backend's `Arc` for dispatch.
    pub fn start_automations(self: &Arc<Self>) {
        self.bind_boss_finish_callback();
        self.start_summon_scheduler();
        self.automations.start(self);
        self.auto_prompts.start(self);
    }

    pub(super) fn bind_boss_finish_callback(self: &Arc<Self>) {
        let backend = Arc::downgrade(self);
        self.boss
            .set_finish_employee(Arc::new(move |session_id, settle| {
                let backend = backend
                    .upgrade()
                    .ok_or_else(|| anyhow::anyhow!("daemon is shutting down"))?;
                // A settled turn drains: the finish defers while the session's
                // next turn is already open rather than cutting it mid-tool-call.
                backend.finish_boss_employee(session_id, true, settle)
            }));
        let agent = self.agent.clone();
        self.boss
            .set_session_busy(Arc::new(move |session_id| agent.has_open_turn(session_id)));
        let backend = Arc::downgrade(self);
        self.boss.set_recover_employee(Arc::new(move |session_id| {
            let Some(backend) = backend.upgrade() else {
                return Ok(());
            };
            backend.recover_boss_employee(session_id)
        }));
        let backend = Arc::downgrade(self);
        self.boss.set_session_active(Arc::new(move |session_id| {
            backend
                .upgrade()
                .map_or(true, |backend| backend.session_active(session_id))
        }));
        let backend = Arc::downgrade(self);
        self.boss.set_archive_sessions(Arc::new(move |sessions| {
            let Some(backend) = backend.upgrade() else {
                return Ok(false);
            };
            backend.archive_sessions(sessions)?;
            Ok(true)
        }));
        let backend = Arc::downgrade(self);
        self.boss.set_project_catalog(Arc::new(move || {
            backend
                .upgrade()
                .map(|backend| backend.task_state.lock().projects.clone())
                .unwrap_or_default()
        }));
    }

    /// Point the `waku-link` ALPN at the daemon's metadata and pairing
    /// service. The handlers are installed once; whichever share runtime
    /// spawns later picks them up.
    pub(super) fn install_link_handlers(&self) {
        let name = self.our_name.clone();
        let share_dir = self.share_dir();
        let daemon_address = self.daemon_address.clone();
        let exposed_port = self.exposed_port.clone();
        let pairing = self.pairing.clone();
        let info: waku_share::link::InfoHandler = Arc::new(move || {
            // Only report a ws port when the daemon is actually bound
            // beyond loopback — otherwise discovery would send clients to
            // an address they cannot reach. A runtime-opened listener wins
            // over the startup bind.
            let ws_port = exposed_port.lock().or_else(|| {
                daemon_address
                    .lock()
                    .as_deref()
                    .and_then(|address| address.parse::<std::net::SocketAddr>().ok())
                    .filter(|address| !address.ip().is_loopback())
                    .map(|address| address.port())
            });
            let endpoint_id = waku_share::identity::load_or_create(&share_dir)
                .map(|secret| secret.public().to_string())
                .unwrap_or_default();
            waku_share::link::DaemonInfo {
                name: name.clone(),
                ws_port,
                protocol_version: waku_protocol::PROTOCOL_VERSION,
                endpoint_id,
            }
        });
        let pair: waku_share::link::PairHandler = Arc::new(move |_id, device_name| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let pairing = pairing.clone();
            // The pairing service is synchronous; park its wait on a
            // blocking thread so the share runtime's executor stays free.
            std::thread::spawn(move || {
                let decision = match pairing.request_blocking(&device_name, "link") {
                    crate::pairing::PairReply::Granted { token } => {
                        waku_share::link::PairDecision::Grant { token }
                    }
                    crate::pairing::PairReply::Declined { .. }
                    | crate::pairing::PairReply::Busy { .. } => {
                        waku_share::link::PairDecision::Decline
                    }
                };
                let _ = tx.send(decision);
            });
            rx
        });
        self.share.set_link_handlers(info, pair);
    }

    pub(super) fn share_dir(&self) -> std::path::PathBuf {
        self.task_store
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("share")
    }

    #[cfg(all(test, unix))]
    pub(crate) fn with_terminal_shell(mut self, shell: alacritty_terminal::tty::Shell) -> Self {
        self.terminal_shell = Some(shell);
        self
    }

    pub(super) fn open_terminal(
        &self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        events: EventSink,
    ) -> anyhow::Result<crate::terminal::DaemonTerminal> {
        #[cfg(all(test, unix))]
        if let Some(shell) = &self.terminal_shell {
            return crate::terminal::DaemonTerminal::open_with_shell(
                cwd,
                cols,
                rows,
                events,
                shell.clone(),
            );
        }
        ensure_shell_environment();
        crate::terminal::DaemonTerminal::open(cwd, cols, rows, events)
    }

    /// Capture and persist one ending checkpoint exactly once per daemon.
    /// Desktop and Web may observe the same turn completion concurrently; a
    /// per-worktree lock prevents both clients — and adjacent turns — from
    /// running the expensive Git snapshot over the same worktree at once
    /// while leaving unrelated worktrees independent.
    pub(super) fn capture_turn_checkpoint(
        &self,
        cwd: PathBuf,
        session_id: Uuid,
        turn_count: usize,
        untouched: bool,
    ) -> anyhow::Result<Checkpoint> {
        crate::checkpoint::with_turn_capture_deadline(|| {
            self.capture_turn_checkpoint_inner(cwd, session_id, turn_count, untouched)
        })
    }

    pub(super) fn capture_turn_checkpoint_inner(
        &self,
        cwd: PathBuf,
        session_id: Uuid,
        turn_count: usize,
        untouched: bool,
    ) -> anyhow::Result<Checkpoint> {
        let capture_lock = self
            .checkpoint_capture_locks
            .lock()
            .entry(cwd.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let capture_lock_started = std::time::Instant::now();
        let _capture = crate::checkpoint::lock_capture_mutex(&capture_lock)?;
        let capture_lock_wait = capture_lock_started.elapsed();

        let dedupe_started = std::time::Instant::now();
        let in_memory_checkpoint = {
            let state = crate::checkpoint::lock_capture_mutex(&self.task_state)?;
            state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| {
                    session
                        .turns
                        .iter()
                        .find(|turn| turn.turn_count == turn_count)
                })
                .and_then(|turn| turn.checkpoint.as_ref())
                .filter(|checkpoint| {
                    matches!(
                        checkpoint.status,
                        CheckpointStatus::Ready | CheckpointStatus::Unavailable
                    )
                })
                .cloned()
        };
        if let Some(checkpoint) = in_memory_checkpoint {
            eprintln!(
                "turn checkpoint session={session_id} turn={turn_count} deduped=true lock_wait={capture_lock_wait:?} lookup_time={:?}",
                dedupe_started.elapsed()
            );
            return Ok(checkpoint);
        }
        // Skeleton sessions have no in-memory turns to inspect. Read their
        // stored detail on its own connection instead of hydrating under the
        // task-state lock, where the disk read would convoy every command.
        if let Some(checkpoint) = self
            .task_store
            .load_turn_checkpoint(session_id, turn_count)?
            .filter(|checkpoint| {
                matches!(
                    checkpoint.status,
                    CheckpointStatus::Ready | CheckpointStatus::Unavailable
                )
            })
        {
            eprintln!(
                "turn checkpoint session={session_id} turn={turn_count} deduped=true lock_wait={capture_lock_wait:?} lookup_time={:?}",
                dedupe_started.elapsed()
            );
            return Ok(checkpoint);
        }
        eprintln!(
            "turn checkpoint session={session_id} turn={turn_count} deduped=false lock_wait={capture_lock_wait:?} lookup_time={:?}",
            dedupe_started.elapsed(),
        );

        let capture_started = std::time::Instant::now();
        let checkpoint = if untouched {
            crate::checkpoint::capture_untouched_turn(&cwd, session_id, turn_count)?
        } else {
            None
        }
        .map_or_else(
            || crate::checkpoint::capture_turn(&cwd, session_id, turn_count),
            Ok,
        )?;
        // The transcript holds a "checking for changes" card open for this
        // round trip — the elapsed line is the only record of how long the
        // worktree snapshot actually took.
        eprintln!(
            "turn checkpoint session={session_id} turn={turn_count} git_capture_time={:?}",
            capture_started.elapsed()
        );
        let persist_started = std::time::Instant::now();
        let checkpoint = self
            .task_store
            .save_turn_checkpoint(session_id, turn_count, &checkpoint)?
            .unwrap_or(checkpoint);
        let mut state = crate::checkpoint::lock_capture_mutex(&self.task_state)?;
        if let Some(session) = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            if let Some(turn) = session
                .turns
                .iter_mut()
                .find(|turn| turn.turn_count == turn_count)
            {
                if let Some(existing) = turn.checkpoint.as_ref().filter(|checkpoint| {
                    matches!(
                        checkpoint.status,
                        CheckpointStatus::Ready | CheckpointStatus::Unavailable
                    )
                }) {
                    // A checkpoint may have landed in memory while this
                    // capture was running. Keep the daemon's terminal value.
                    return Ok(existing.clone());
                }
                turn.checkpoint = Some(checkpoint.clone());
            }
        }
        drop(state);
        eprintln!(
            "turn checkpoint session={session_id} turn={turn_count} persist_total={:?}",
            persist_started.elapsed()
        );
        Ok(checkpoint)
    }

    /// Drop terminals whose owning task is gone or whose cwd sat under a
    /// removed workspace. Remote clients send `CloseTerminal` when their
    /// surfaces unmount, but a disconnect — or a deletion another client
    /// made — skips that, and the PTY plus its shell would otherwise run
    /// until daemon exit.
    pub(super) fn sweep_orphaned_terminals(
        &self,
        removed_sessions: &[Uuid],
        workspace_roots: &[PathBuf],
    ) -> Vec<crate::terminal::DaemonTerminal> {
        let mut terminals = self.terminals.lock();
        let orphaned = terminals
            .iter()
            .filter(|(_, entry)| {
                entry
                    .owner
                    .is_some_and(|owner| removed_sessions.contains(&owner))
                    || workspace_roots
                        .iter()
                        .any(|root| entry.cwd.starts_with(root))
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let events = self.event_source.lock();
        for id in &orphaned {
            events.release_terminal_channel(*id);
        }
        orphaned
            .into_iter()
            .filter_map(|id| terminals.remove(&id).map(|entry| entry.terminal))
            .collect()
    }

    /// Removes one task from daemon state and storage. The id is remembered
    /// so a stale client `SaveTaskState` cannot restore the row, and any live
    /// runtime is dropped with it. When the departed task was the last one in
    /// a projectless workspace, that workspace leaves with it — its live
    /// directory and any archive zip — since no session can reach it again.
    pub(super) fn remove_session(&self, session_id: Uuid) -> anyhow::Result<()> {
        let mut removed_workspace = None;
        let mut removed_ids;
        let mut workspace_roots: Vec<PathBuf>;
        {
            let mut state = self.task_state.lock();
            // Side chats die with their parent: walk the descendant set
            // first so a removal can never leave orphans behind.
            removed_ids = vec![session_id];
            let mut cursor = 0;
            while cursor < removed_ids.len() {
                let parent = removed_ids[cursor];
                cursor += 1;
                removed_ids.extend(
                    state
                        .sessions
                        .iter()
                        .filter(|session| session.side_chat_of == Some(parent))
                        .map(|session| session.id),
                );
            }
            {
                let mut removed = self.removed_session_ids.lock();
                for id in &removed_ids {
                    removed.insert(*id);
                }
            }
            // Terminals can't be attributed to a session row once it's gone,
            // so gather each removed task's dedicated workspace first. The
            // shared project root is deliberately absent — sibling tasks
            // keep their terminals.
            workspace_roots = state
                .sessions
                .iter()
                .filter(|session| removed_ids.contains(&session.id))
                .filter_map(|session| session.workspace.path().map(Path::to_path_buf))
                .collect();
            let project_id = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| session.project_id);
            state
                .sessions
                .retain(|session| !removed_ids.contains(&session.id));
            if let Some(project_id) = project_id {
                let remove_project = state
                    .projects
                    .iter()
                    .find(|project| project.id == project_id)
                    .is_some_and(Project::is_projectless)
                    && !state
                        .sessions
                        .iter()
                        .any(|session| session.project_id == project_id);
                if remove_project {
                    removed_workspace = state
                        .projects
                        .iter()
                        .find(|project| project.id == project_id)
                        .map(|project| project.path.clone());
                    state.projects.retain(|project| project.id != project_id);
                }
            }
            self.task_store.save(&mut state)?;
        }
        if let Some(path) = removed_workspace {
            workspace_roots.push(path.clone());
            // A failed removal leaves files behind — safe — so the task's
            // removal does not hinge on it.
            let _ = crate::projectless::remove_workspace(&path);
        }
        let removed_terminals = self.sweep_orphaned_terminals(&removed_ids, &workspace_roots);
        drop_detached(removed_terminals);
        let removed_runtimes = removed_ids
            .iter()
            .filter_map(|id| self.sessions.lock().remove(id))
            .collect::<Vec<_>>();
        for runtime in &removed_runtimes {
            runtime.driver.begin_shutdown();
        }
        drop_detached(removed_runtimes);
        for id in removed_ids {
            self.agent.clear_session(id);
        }
        Ok(())
    }

    /// Removes a project and every task under it. The ids are remembered for
    /// the same reason `remove_session` remembers tasks: a stale client save
    /// must not restore the catalog row another client just deleted.
    pub(super) fn remove_project(&self, project_id: Uuid) -> anyhow::Result<()> {
        let mut removed_workspace = None;
        let mut removed_ids;
        let workspace_roots: Vec<PathBuf>;
        {
            let mut state = self.task_state.lock();
            let Some(index) = state
                .projects
                .iter()
                .position(|project| project.id == project_id)
            else {
                return Ok(());
            };
            if state.projects[index].is_projectless() {
                removed_workspace = Some(state.projects[index].path.clone());
            }
            let mut removed_set = state
                .sessions
                .iter()
                .filter(|session| session.project_id == project_id)
                .map(|session| session.id)
                .collect::<HashSet<_>>();
            let mut cursor = 0;
            removed_ids = removed_set.iter().copied().collect::<Vec<_>>();
            while cursor < removed_ids.len() {
                let parent = removed_ids[cursor];
                cursor += 1;
                for child in state
                    .sessions
                    .iter()
                    .filter(|session| session.side_chat_of == Some(parent))
                    .map(|session| session.id)
                {
                    if removed_set.insert(child) {
                        removed_ids.push(child);
                    }
                }
            }
            self.removed_project_ids.lock().insert(project_id);
            {
                let mut removed = self.removed_session_ids.lock();
                for id in &removed_ids {
                    removed.insert(*id);
                }
            }
            // The project row is leaving the catalog entirely: its checkout
            // and every removed task's worktree both end as sweep roots.
            workspace_roots = std::iter::once(state.projects[index].path.clone())
                .chain(
                    state
                        .sessions
                        .iter()
                        .filter(|session| removed_ids.contains(&session.id))
                        .filter_map(|session| session.workspace.path().map(Path::to_path_buf)),
                )
                .collect();
            state
                .sessions
                .retain(|session| !removed_ids.contains(&session.id));
            state.projects.remove(index);
            self.task_store.save(&mut state)?;
        }
        if let Some(path) = removed_workspace {
            let _ = crate::projectless::remove_workspace(&path);
        }
        let removed_terminals = self.sweep_orphaned_terminals(&removed_ids, &workspace_roots);
        drop_detached(removed_terminals);
        let removed_runtimes = removed_ids
            .iter()
            .filter_map(|id| self.sessions.lock().remove(id))
            .collect::<Vec<_>>();
        for runtime in &removed_runtimes {
            runtime.driver.begin_shutdown();
        }
        drop_detached(removed_runtimes);
        for id in removed_ids {
            self.agent.clear_session(id);
        }
        Ok(())
    }

    /// Applies a user-approved archive proposal: flag the named tasks and
    /// retire their runtimes, exactly what a client's own archive does to
    /// daemon state. Side chats leave with their parent — deleted, as the
    /// save path's cascade enforces — and every attached client learns the
    /// change through the task-state bump the request returns under. Also
    /// the planning-grace sweep's archive path: `BossService` reaches it
    /// through its bound backend handle.
    pub(crate) fn archive_sessions(&self, session_ids: &[Uuid]) -> anyhow::Result<()> {
        let mut archived_ids = Vec::new();
        let mut removed_ids;
        let workspace_roots: Vec<PathBuf>;
        {
            let mut state = self.task_state.lock();
            let now = crate::model::unix_time();
            for id in session_ids {
                let Some(session) = state.sessions.iter().find(|session| {
                    session.id == *id
                        && session.archived_at.is_none()
                        && session.side_chat_of.is_none()
                }) else {
                    continue;
                };
                if let Some((path, reason)) = WakuBackend::employee_archive_blocker(session)? {
                    // Bulk requests archive per task so one dirty employee
                    // does not prevent clean tasks in the same request.
                    if session_ids.len() == 1 {
                        bail!("employee has {reason} in {}", path.display());
                    }
                    continue;
                }
                let session = state
                    .session_mut(*id)
                    .expect("archivable session is present");
                session.archived_at = Some(now);
                // Bumping `updated_at` keeps merge precedence honest — the
                // same guard the client's archive applies so a stale save
                // cannot resurrect or clobber the flag.
                session.updated_at = now;
                archived_ids.push(*id);
            }
            if archived_ids.is_empty() {
                return Ok(());
            }
            // Archiving a task deletes its side chats. Roots are every
            // archived row, not just this call's, so a side chat whose
            // parent was already archived is swept too. The roots stay;
            // only the descendants found walking them are removed.
            let mut queue: Vec<Uuid> = state
                .sessions
                .iter()
                .filter(|session| session.archived_at.is_some())
                .map(|session| session.id)
                .collect();
            removed_ids = Vec::new();
            while let Some(parent) = queue.pop() {
                for session in state
                    .sessions
                    .iter()
                    .filter(|session| session.side_chat_of == Some(parent))
                {
                    removed_ids.push(session.id);
                    queue.push(session.id);
                }
            }
            workspace_roots = state
                .sessions
                .iter()
                .filter(|session| removed_ids.contains(&session.id))
                .filter_map(|session| session.workspace.path().map(Path::to_path_buf))
                .collect();
            if !removed_ids.is_empty() {
                state
                    .sessions
                    .retain(|session| !removed_ids.contains(&session.id));
                let mut removed = self.removed_session_ids.lock();
                for id in &removed_ids {
                    removed.insert(*id);
                }
            }
            self.task_store.save(&mut state)?;
        }
        let removed_terminals = self.sweep_orphaned_terminals(&removed_ids, &workspace_roots);
        drop_detached(removed_terminals);
        // An archived task keeps no live runtime — the same outcome as the
        // client's archive evicting it — and a removed side chat's dies too.
        let gone: Vec<Uuid> = archived_ids.iter().chain(&removed_ids).copied().collect();
        let removed_runtimes = gone
            .iter()
            .filter_map(|id| self.sessions.lock().remove(id))
            .collect::<Vec<_>>();
        for runtime in &removed_runtimes {
            runtime.driver.begin_shutdown();
        }
        drop_detached(removed_runtimes);
        for id in &archived_ids {
            self.agent.revoke_session(*id);
        }
        for id in removed_ids {
            self.agent.clear_session(id);
        }
        Ok(())
    }

    /// Return the reason and path when archiving would discard employee work.
    /// The daemon owns this check so every archive surface applies the rule.
    pub(super) fn employee_archive_blocker(
        session: &AgentSession,
    ) -> anyhow::Result<Option<(PathBuf, &'static str)>> {
        if !session.boss_managed {
            return Ok(None);
        }
        let SessionWorkspace::Worktree {
            path,
            base_branch,
            adopted_by: None,
            ..
        } = &session.workspace
        else {
            return Ok(None);
        };

        let status = crate::git_commit::git_stdout(
            path,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?;
        if !status.is_empty() {
            return Ok(Some((path.clone(), "uncommitted work")));
        }

        let Some(base) = crate::git_panel::land_base(path, base_branch.as_deref())? else {
            return Ok(Some((path.clone(), "unlanded commits")));
        };
        let range = format!("{base}..HEAD");
        let ahead = crate::git_commit::git_optional_stdout(path, &["rev-list", "--count", &range])?
            .and_then(|count| count.trim().parse::<u64>().ok())
            .unwrap_or(0);
        Ok((ahead > 0).then(|| (path.clone(), "unlanded commits")))
    }

    /// Deletes archived tasks whose archive has outlived the retention
    /// window.
    ///
    /// Runs whenever task state loads — daemon startup and every client
    /// `LoadTaskState` — so retention does not depend on a timer inside a
    /// daemon clients may keep alive for weeks. As with ordinary removal, the
    /// task's Git worktree is deliberately left on disk; only its checkpoint
    /// refs are deleted. A projectless workspace does leave with its last
    /// task — `remove_session` drops the live directory and any archive zip.
    pub(super) fn purge_expired_archived_sessions(&self) {
        let cutoff = crate::model::unix_time().saturating_sub(ARCHIVED_SESSION_RETENTION_SECONDS);
        let expired = {
            let state = self.task_state.lock();
            let mut expired = Vec::new();
            for index in 0..state.sessions.len() {
                if state.sessions[index]
                    .archived_at
                    .is_none_or(|archived_at| archived_at > cutoff)
                {
                    continue;
                }
                let session_id = state.sessions[index].id;
                // Checkpoint refs live in the repository's shared
                // namespace, so delete them from the project checkout —
                // the task's worktree may already be gone, and a missing
                // cwd would silently leave the refs behind.
                let workspace = state
                    .projects
                    .iter()
                    .find(|project| project.id == state.sessions[index].project_id)
                    .map(|project| project.path.clone())
                    .or_else(|| {
                        state.sessions[index]
                            .workspace
                            .path()
                            .map(Path::to_path_buf)
                    });
                expired.push((session_id, workspace));
            }
            expired
        };
        for (session_id, workspace) in expired {
            if let Some(cwd) = workspace {
                let _ = crate::checkpoint::delete_all_session_refs(&cwd, session_id);
            }
            let _ = self.remove_session(session_id);
        }
    }

    /// Strips heavyweight transcript payloads from detail rows archived past
    /// [`ARCHIVED_DETAIL_RETENTION_SECONDS`]. Scheduled from the same places
    /// as the full archive purge — startup and every `LoadTaskState` — but
    /// runs on its own thread in small committed batches: parsing and
    /// rewriting hundreds of megabytes of session JSON is far too slow for
    /// the request path that triggers it.
    pub(super) fn start_archive_detail_prune(&self) {
        use std::sync::atomic::Ordering;
        if self
            .archive_detail_prune_running
            .swap(true, Ordering::AcqRel)
        {
            return;
        }
        let store = Arc::clone(&self.task_store);
        let boss = Arc::clone(&self.boss);
        let running = Arc::clone(&self.archive_detail_prune_running);
        let _ = std::thread::Builder::new()
            .name("waku-archive-prune".to_owned())
            .spawn(move || {
                let cutoff =
                    crate::model::unix_time().saturating_sub(ARCHIVED_DETAIL_RETENTION_SECONDS);
                loop {
                    // Read per batch: the Boss document only exists once the
                    // experiment activates, and an identity read before then
                    // would prune chats the exemption covers.
                    let boss_project = boss.document().identity.id;
                    match store.prune_archived_session_details(
                        cutoff,
                        ARCHIVE_PRUNE_BATCH,
                        boss_project,
                    ) {
                        Ok(0) => break,
                        Ok(_) => std::thread::yield_now(),
                        Err(error) => {
                            eprintln!("archive detail prune failed: {error:#}");
                            break;
                        }
                    }
                }
                running.store(false, Ordering::Release);
            });
    }
}
