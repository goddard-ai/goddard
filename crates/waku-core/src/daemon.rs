//! Provider backend and driver-event wire translation for `goddard-daemon`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::{
    AgentPromptDelivery, AgentWorkspace, Backend, Command, EventSink, Request, ResponsePayload,
    WireDriverEvent, WorkspaceOperation, WorkspaceResult,
};
use anyhow::{Context as _, anyhow, bail};
use parking_lot::Mutex;
use serde_json::Value;
use uuid::Uuid;

use crate::attachments::AttachmentStore;
use crate::driver::{self, DriverHandle, DriverStartOptions, SessionOptions};
use crate::model::{
    AgentSession, Checkpoint, CheckpointStatus, DriverEvent,
    Project, ProviderKind, ProviderResumeCursor, SessionStatus, SessionWorkspace, TurnStatus,
};
use crate::persistence::{ComposerDraftStore, PersistedState, StateStore};
use crate::settings::DaemonSettingsStore;
use waku_protocol::custom_commands::CustomCommand;
use waku_protocol::provider_session::{ProviderSessionFork, ProviderSessionForkRequest};
use waku_protocol::routing::RoutePolicyView;
use waku_protocol::{decode_enum, event_to_wire};
#[cfg(test)]
use serde_json::json;
#[cfg(test)]
use waku_protocol::event_from_wire;

/// How many fully hydrated transcripts the daemon keeps resident.
///
/// Hydration is a cache: consumers reload a released session from the store on
/// demand. Without a cap, a daemon that lives for days adopts the transcript
/// of every session its clients have touched — SaveTaskState pushes, hydrate
/// requests, forks, checkpoints — and resident memory grows without bound.
const RESIDENT_TRANSCRIPT_WINDOW: usize = 24;

/// How long an archived task is kept, in seconds, before it is removed
/// entirely. The sweep runs whenever task state loads rather than on a
/// timer, so this bounds retention without scheduling exact deletions.
const ARCHIVED_SESSION_RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;

/// Releases resident transcripts beyond the recency window after a save.
/// `pinned` names sessions with live runtimes; dirty sessions are skipped
/// inside [`PersistedState::trim_idle_transcripts`] because they hold unsaved
/// work.
fn trim_resident_transcripts(state: &mut PersistedState, pinned: &HashSet<Uuid>) {
    state.trim_idle_transcripts(pinned, RESIDENT_TRANSCRIPT_WINDOW);
}

pub struct WakuBackend {
    sessions: Arc<Mutex<HashMap<Uuid, (Uuid, DriverHandle)>>>,
    terminals: Mutex<HashMap<Uuid, (Uuid, crate::terminal::DaemonTerminal)>>,
    #[cfg(all(test, unix))]
    terminal_shell: Option<alacritty_terminal::tty::Shell>,
    settings: DaemonSettingsStore,
    task_store: Arc<StateStore>,
    task_state: Arc<Mutex<PersistedState>>,
    removed_session_ids: Mutex<HashSet<Uuid>>,
    composer_drafts: ComposerDraftStore,
    attachments: AttachmentStore,
    usage_scan_cache: Mutex<crate::usage_history::ScanCache>,
    checkpoint_capture_locks: Mutex<HashMap<(PathBuf, Uuid, usize), Arc<Mutex<()>>>>,
    /// Scoped agent credentials, per-session prompt queues, and the live
    /// turn bookkeeping the runtime event forwarder maintains. `pub(crate)`
    /// so server tests can mint a token and connect with it the way a
    /// provider session's harness would.
    pub(crate) agent: Arc<crate::agent::AgentState>,
    /// Serializes cold-start of a stored task's runtime so two agent prompts
    /// cannot race to spawn it.
    runtime_start_locks: Mutex<HashMap<Uuid, Arc<Mutex<()>>>>,
    /// The address the daemon bound, published to provider sessions as
    /// `GODDARD_DAEMON_ADDRESS` when agent tools are enabled. Set once by the
    /// daemon executable after it binds its listener.
    daemon_address: Mutex<Option<String>>,
    usage_rates_dir: std::path::PathBuf,
    default_cwd: std::path::PathBuf,
    /// Friend-to-friend sharing; lazily binds the iroh endpoint on first
    /// friends command so tests and headless runs pay nothing.
    share: Arc<crate::share::ShareService>,
    /// The hot-reloading view of the user's `route-policy.json`.
    route_policy: crate::route_policy::PolicyStore,
}

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
        let share_dir = data_dir.join("share");
        let our_name = std::env::var("USER")
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Goddard".to_owned());
        let usage_rates_dir = data_dir.clone();
        let route_policy =
            crate::route_policy::PolicyStore::open(data_dir.join("route-policy.json"));
        let backend = Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            terminals: Mutex::new(HashMap::new()),
            #[cfg(all(test, unix))]
            terminal_shell: None,
            settings,
            task_store: Arc::new(task_store),
            task_state: Arc::new(Mutex::new(task_state)),
            removed_session_ids: Mutex::new(HashSet::new()),
            composer_drafts,
            attachments,
            usage_scan_cache: Mutex::new(HashMap::new()),
            checkpoint_capture_locks: Mutex::new(HashMap::new()),
            agent: Arc::new(crate::agent::AgentState::default()),
            runtime_start_locks: Mutex::new(HashMap::new()),
            daemon_address: Mutex::new(None),
            usage_rates_dir,
            route_policy,
            default_cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            share: Arc::new(crate::share::ShareService::new(share_dir.clone(), our_name)),
        };
        backend.purge_expired_archived_sessions();
        {
            let task_state = backend.task_state.clone();
            let task_store = backend.task_store.clone();
            let share_dir = share_dir.clone();
            backend.share.set_transfer_hook(Arc::new(move |transfer| {
                match create_transfer_session(&task_state, &task_store, &share_dir, transfer) {
                    Ok(session_id) => Some(session_id),
                    Err(error) => {
                        eprintln!("could not create transfer session: {error:#}");
                        None
                    }
                }
            }));
        }
        Ok(backend)
    }

    /// Record the daemon's bound address for `GODDARD_DAEMON_ADDRESS`
    /// injection. Called once by the daemon executable before it starts
    /// serving; providers launched while it is unset get no agent surface.
    pub fn set_daemon_address(&self, address: String) {
        *self.daemon_address.lock() = Some(address);
    }

    #[cfg(all(test, unix))]
    pub(crate) fn with_terminal_shell(mut self, shell: alacritty_terminal::tty::Shell) -> Self {
        self.terminal_shell = Some(shell);
        self
    }

    fn open_terminal(
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
    /// per-turn lock prevents both clients from running the expensive Git
    /// snapshot while leaving unrelated tasks independent.
    fn capture_turn_checkpoint(
        &self,
        cwd: PathBuf,
        session_id: Uuid,
        turn_count: usize,
    ) -> anyhow::Result<Checkpoint> {
        let key = (cwd.clone(), session_id, turn_count);
        let capture_lock = self
            .checkpoint_capture_locks
            .lock()
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _capture = capture_lock.lock();

        {
            let mut state = self.task_state.lock();
            if let Some(index) = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
            {
                self.task_store.hydrate(&mut state.sessions[index])?;
                if let Some(checkpoint) = state.sessions[index]
                    .turns
                    .iter()
                    .find(|turn| turn.turn_count == turn_count)
                    .and_then(|turn| turn.checkpoint.as_ref())
                    .filter(|checkpoint| {
                        matches!(
                            checkpoint.status,
                            CheckpointStatus::Ready | CheckpointStatus::Unavailable
                        )
                    })
                {
                    return Ok(checkpoint.clone());
                }
            }
        }

        let checkpoint = crate::checkpoint::capture_turn(&cwd, session_id, turn_count)?;
        let mut state = self.task_state.lock();
        if let Some(index) = state
            .sessions
            .iter()
            .position(|session| session.id == session_id)
        {
            self.task_store.hydrate(&mut state.sessions[index])?;
            if let Some(turn) = state.sessions[index]
                .turns
                .iter_mut()
                .find(|turn| turn.turn_count == turn_count)
            {
                turn.checkpoint = Some(checkpoint.clone());
                state.mark_session_dirty(session_id);
                self.task_store.save(&mut state)?;
            }
        }
        Ok(checkpoint)
    }

    /// Removes one task from daemon state and storage. The id is remembered
    /// so a stale client `SaveTaskState` cannot restore the row, and any live
    /// runtime is dropped with it. When the departed task was the last one in
    /// a projectless workspace, that workspace leaves with it — its live
    /// directory and any archive zip — since no session can reach it again.
    fn remove_session(&self, session_id: Uuid) -> anyhow::Result<()> {
        let mut removed_workspace = None;
        {
            let mut state = self.task_state.lock();
            self.removed_session_ids.lock().insert(session_id);
            let project_id = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| session.project_id);
            state.sessions.retain(|session| session.id != session_id);
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
            // A failed removal leaves files behind — safe — so the task's
            // removal does not hinge on it.
            let _ = crate::projectless::remove_workspace(&path);
        }
        let removed = self.sessions.lock().remove(&session_id);
        drop(removed);
        self.agent.clear_session(session_id);
        Ok(())
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
    fn purge_expired_archived_sessions(&self) {
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
}

/// Storage-layout migrations belong to the daemon because both the database
/// rows and the directories name paths on its host. Persist after each move
/// so a later failure cannot leave an earlier project pointing at its old
/// location in SQLite.
fn migrate_projectless_state(
    task_store: &StateStore,
    task_state: &mut PersistedState,
) -> anyhow::Result<()> {
    let indices = task_state
        .projects
        .iter()
        .enumerate()
        .filter_map(|(index, project)| {
            crate::projectless::needs_migration(&project.path).then_some(index)
        })
        .collect::<Vec<_>>();
    for index in indices {
        let old_path = task_state.projects[index].path.clone();
        let workspace = crate::projectless::migrate_workspace(&old_path).with_context(|| {
            format!(
                "could not move projectless workspace {} under ~/.goddard/projects",
                old_path.display()
            )
        })?;
        task_state.projects[index].name = crate::model::Project::PROJECTLESS_NAME.to_owned();
        task_state.projects[index].path = workspace.cwd;
        task_store
            .save(task_state)
            .context("could not persist migrated projectless workspace")?;
    }
    Ok(())
}

/// Materialize a transfer's agent session: a task under the synthetic
/// "Friends" project whose first message is the receipt — peer, title,
/// sender note, and where the files landed. The session stays quarantined
/// (idle, no turn started) until the user chooses to trust it.
fn create_transfer_session(
    task_state: &Arc<Mutex<PersistedState>>,
    task_store: &Arc<StateStore>,
    share_dir: &Path,
    transfer: &waku_protocol::friends::TransferInfo,
) -> anyhow::Result<Uuid> {
    let Some(dest_dir) = &transfer.dest_dir else {
        bail!("incoming transfer completed without a destination");
    };
    let mut state = task_state.lock();
    let project_id = match state
        .projects
        .iter()
        .find(|project| project.path == share_dir)
        .map(|project| project.id)
    {
        Some(id) => id,
        None => {
            let mut project = Project::from_path(share_dir.to_path_buf());
            project.name = "Friends".to_owned();
            let id = project.id;
            state.projects.push(project);
            id
        }
    };
    let mut session = AgentSession::new(project_id, ProviderKind::Claude);
    session.title = format!("{} from {}", transfer.title, transfer.peer_name);
    let mut receipt = format!(
        "{} sent you \"{}\".",
        transfer.peer_name, transfer.title
    );
    if let Some(note) = transfer.note.as_deref().filter(|note| !note.is_empty()) {
        receipt.push_str(&format!("\n\n{note}"));
    }
    receipt.push_str(&format!(
        "\n\nFiles are in {}\n\nThe files have not been opened or executed — decide whether you trust them before asking me to work with them.",
        dest_dir.display()
    ));
    session.adopt_submitted_prompt(&receipt, Uuid::new_v4(), Uuid::new_v4(), None, false);
    // The receipt is a notification, not a turn awaiting a reply — close it
    // out so the session renders Idle instead of an eternal spinner.
    let now = crate::model::unix_time();
    if let Some(turn) = session.turns.last_mut() {
        turn.status = TurnStatus::Completed;
        turn.completed_at = Some(now);
    }
    session.status = SessionStatus::Idle;
    session.quarantined = true;
    let session_id = session.id;
    state.push_session(session);
    task_store.save(&mut state)?;
    Ok(session_id)
}

impl Backend for WakuBackend {
    fn authenticate_agent(&self, token: &str) -> Option<Uuid> {
        self.agent.resolve(token)
    }

    fn set_friends_sink(&self, sink: crate::share::FriendsSink) {
        self.share.set_sink(sink);
    }

    fn set_task_state_sink(&self, sink: crate::share::TaskNotifier) {
        self.share.set_task_notifier(sink);
    }

    fn handle(
        &self,
        request: Request,
        events: EventSink,
        agent: Option<Uuid>,
    ) -> anyhow::Result<ResponsePayload> {
        let session_id = request.session_id;
        let runtime_id = request.runtime_id;
        match request.command {
            Command::AttachSession => {
                let sessions = self.sessions.lock();
                let Some((runtime_id, driver)) = sessions.get(&session_id) else {
                    return Ok(ResponsePayload::SessionRuntime {
                        runtime_id: None,
                        supports_steer: false,
                        supports_user_input_actions: false,
                    });
                };
                Ok(ResponsePayload::SessionRuntime {
                    runtime_id: Some(*runtime_id),
                    supports_steer: driver.supports_steer(),
                    supports_user_input_actions: driver.supports_user_input_actions(),
                })
            }
            Command::GetSettings => Ok(ResponsePayload::Settings {
                settings: self.settings.get(),
            }),
            Command::GetFriends => {
                // Reading friends state means this install wants to be
                // reachable — incoming requests and offers can only
                // arrive while the endpoint is up.
                self.share.kickstart();
                Ok(ResponsePayload::Friends {
                    state: self.share.state(),
                })
            }
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
                note,
            } => {
                self.share.send_file(node_id, path, note)?;
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
            Command::UpdateSettings { settings } => {
                self.settings.replace(settings)?;
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
            Command::ProbeProvider {
                provider,
                binary_override,
                discover_models,
                probe_version,
            } => {
                ensure_shell_environment();
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
                    _ => bail!("provider has no plan usage fetcher"),
                };
                Ok(ResponsePayload::PlanUsage { usage })
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
            Command::Evaluate { state, questions } => {
                let settings = self
                    .settings
                    .get()
                    .eval
                    .ok_or_else(|| anyhow!("no evaluation backend is configured"))?;
                let started = std::time::Instant::now();
                let result = crate::eval::evaluate(&settings, &state, &questions);
                let mut record = crate::eval::EvalDecisionRecord::empty("evaluate");
                record.backend = Some(settings.backend);
                record.latency_ms = Some(started.elapsed().as_millis() as u64);
                record.model = result
                    .as_ref()
                    .ok()
                    .map(|evaluation| evaluation.model.clone());
                record.state = Some(state);
                record.questions = Some(questions);
                record.answers = result
                    .as_ref()
                    .ok()
                    .map(|evaluation| evaluation.answers.clone());
                record.error = result.as_ref().err().map(|error| error.to_string());
                crate::eval::append_decision_log(&crate::eval::default_log_path(), &record);
                Ok(ResponsePayload::Evaluation {
                    evaluation: result?,
                })
            }
            Command::RouteTask {
                prompt,
                project,
                candidates,
                last_used,
            } => {
                let settings = self.settings.get();
                let policy = self.route_policy.get();
                let run = crate::routing::route_task(
                    settings.eval.as_ref(),
                    &policy,
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
            Command::GetRoutePolicy => {
                let policy = self.route_policy.get();
                Ok(ResponsePayload::RoutePolicy {
                    view: RoutePolicyView {
                        path: self.route_policy.path().to_path_buf(),
                        valid: !policy.is_default,
                        hash: policy.hash.clone(),
                        classes: policy.classes_raw.clone(),
                        default: policy.default_raw.clone(),
                    },
                })
            }
            Command::SetRouteClassTarget { class, target } => {
                self.route_policy.set_class_target(class, &target)?;
                Ok(ResponsePayload::Ack)
            }
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
                let state = self.task_state.lock();
                Ok(ResponsePayload::TaskState {
                    projects: state.projects.clone(),
                    sessions: state
                        .sessions
                        .iter()
                        .filter(|session| session.has_started())
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
            } => {
                let active_runtimes = self
                    .sessions
                    .lock()
                    .iter()
                    .map(|(session_id, (runtime_id, _))| (*session_id, *runtime_id))
                    .collect::<HashMap<_, _>>();
                let mut state = self.task_state.lock();
                let removed_session_ids = self.removed_session_ids.lock();
                for project in projects {
                    if let Some(existing) = state
                        .projects
                        .iter_mut()
                        .find(|existing| existing.id == project.id)
                    {
                        *existing = project;
                    } else {
                        state.projects.push(project);
                    }
                }
                let sessions = sessions
                    .into_iter()
                    .filter(|session| !removed_session_ids.contains(&session.id))
                    .collect::<Vec<_>>();
                drop(removed_session_ids);
                let mut saved_ids = Vec::with_capacity(sessions.len());
                for mut session in sessions {
                    let session_id = session.id;
                    let applied = if let Some(existing) = state
                        .sessions
                        .iter_mut()
                        .find(|existing| existing.id == session_id)
                    {
                        if !session.detail_loaded {
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
                state.projects.retain(|project| {
                    !project.is_projectless() || used_project_ids.contains(&project.id)
                });
                for session_id in &saved_ids {
                    state.mark_session_dirty(*session_id);
                }
                self.task_store.save(&mut state)?;
                let sessions = saved_ids
                    .into_iter()
                    .filter_map(|session_id| {
                        state
                            .sessions
                            .iter()
                            .find(|session| session.id == session_id)
                            .cloned()
                    })
                    .collect();
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
            Command::HydrateSession { session_id } => {
                // Live runtimes stay resident; everything else is trimmed to
                // the recency window once the response is built.
                let pinned = self.sessions.lock().keys().copied().collect();
                let mut state = self.task_state.lock();
                let session = if let Some(session) = state
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                {
                    self.task_store.hydrate(session)?;
                    Some(session.clone())
                } else {
                    None
                };
                trim_resident_transcripts(&mut state, &pinned);
                Ok(ResponsePayload::Session { session })
            }
            Command::SearchSessionMessages { query, limit } => {
                let matches = self.task_store.session_message_search(query, limit)()?;
                Ok(ResponsePayload::SessionMessageMatches { matches })
            }
            Command::ListProviderSessions { provider, limit } => {
                const MAX_PROVIDER_SESSIONS: usize = 500;
                let limit = limit.min(MAX_PROVIDER_SESSIONS);
                if limit == 0 {
                    return Ok(ResponsePayload::ProviderSessions {
                        sessions: Vec::new(),
                    });
                }
                ensure_shell_environment();
                let settings = self.settings.get();
                if settings.disabled_providers.contains(&provider) {
                    return Ok(ResponsePayload::ProviderSessions {
                        sessions: Vec::new(),
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
                    });
                };
                // Discovery is deliberately provider-scoped. Opening Resume
                // must not start every installed agent CLI, and another
                // provider is queried only after the user explicitly picks it.
                let mut sessions = match provider {
                    // Antigravity conversations live in its own TUI; there is
                    // no Goddard transcript to import.
                    ProviderKind::Antigravity => Vec::new(),
                    ProviderKind::Amp => {
                        crate::amp_session::list_provider_sessions(&binary, limit)?
                    }
                    ProviderKind::Claude => crate::claude_session::list_provider_sessions(limit)?,
                    ProviderKind::Codex => {
                        crate::codex_session::list_provider_sessions(&binary, limit)?
                    }
                    ProviderKind::Copilot => {
                        crate::copilot_session::list_provider_sessions(limit)?
                    }
                    ProviderKind::Cursor
                    | ProviderKind::Devin
                    | ProviderKind::Fx
                    | ProviderKind::Droid => {
                        crate::acp_session::list_provider_sessions(provider, &binary, &[], limit)?
                    }
                    ProviderKind::OpenCode => {
                        crate::opencode_session::list_provider_sessions(&binary, limit)?
                    }
                    ProviderKind::OpenCode2 => {
                        crate::opencode2_session::list_provider_sessions(&binary, limit)?
                    }
                    ProviderKind::DeepSeek => {
                        crate::deepseek_session::list_provider_sessions(&binary, limit)?
                    }
                    ProviderKind::Grok => crate::grok_session::list_provider_sessions(limit)?,
                    ProviderKind::Kimi => crate::kimi_session::list_provider_sessions(limit)?,
                    ProviderKind::Muse => {
                        crate::muse_session::list_provider_sessions(&binary, limit)?
                    }
                    ProviderKind::OhMyPi | ProviderKind::Pi => {
                        crate::pi_session::list_provider_sessions(provider, limit)?
                    }
                };
                sessions.sort_by(|a, b| {
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
                sessions.retain(|session| {
                    !imported.contains(&(session.provider(), session.cursor.native_id().to_owned()))
                });
                sessions.truncate(limit);
                Ok(ResponsePayload::ProviderSessions { sessions })
            }
            Command::LoadProviderSession { cursor, cwd } => {
                // Preserve every native turn shell for exact provider turn
                // numbering, but bound imported display text to recent turns.
                const VISIBLE_TURN_LIMIT: usize = 100;
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
                    // OpenCode 2 is not an ACP provider: its history comes
                    // from the adopted v2 service's own export route.
                    ProviderResumeCursor::OpenCode2 { session_id, .. } => {
                        let binary = self.provider_binary(ProviderKind::OpenCode2)?;
                        crate::opencode2_session::provider_session_history(
                            &binary,
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Cursor { session_id, .. }
                    | ProviderResumeCursor::Devin { session_id }
                    | ProviderResumeCursor::Fx { session_id }
                    | ProviderResumeCursor::OpenCode { session_id }
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
                        bail!("Antigravity conversations live in its own TUI; there is no transcript to import")
                    }
                };
                Ok(ResponsePayload::ProviderSessionHistory { history })
            }
            Command::LoadComposerDrafts => Ok(ResponsePayload::ComposerDrafts {
                drafts: self.composer_drafts.load()?,
            }),
            Command::SaveComposerDrafts { drafts, generation } => {
                self.composer_drafts.save(drafts, generation)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ApplyComposerDraftChanges { changes } => {
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
                    },
            } => Ok(ResponsePayload::Workspace {
                result: WorkspaceResult::Checkpoint {
                    checkpoint: self.capture_turn_checkpoint(cwd, session_id, turn_count)?,
                },
            }),
            Command::Workspace { operation } => Ok(ResponsePayload::Workspace {
                result: crate::workspace::execute(operation)?,
            }),
            Command::OpenTerminal { cwd, cols, rows } => {
                let terminal = self.open_terminal(&cwd, cols, rows, events)?;
                let previous = self
                    .terminals
                    .lock()
                    .insert(session_id, (runtime_id, terminal));
                drop(previous);
                Ok(ResponsePayload::Ack)
            }
            Command::WriteTerminal { data } => {
                let terminals = self.terminals.lock();
                let (active_runtime_id, terminal) = terminals
                    .get(&session_id)
                    .ok_or_else(|| anyhow!("daemon terminal {session_id} is not running"))?;
                if *active_runtime_id != runtime_id {
                    bail!(
                        "daemon terminal {session_id} belongs to runtime {active_runtime_id}, not {runtime_id}"
                    );
                }
                terminal.write(data)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ResizeTerminal { cols, rows } => {
                let terminals = self.terminals.lock();
                let (active_runtime_id, terminal) = terminals
                    .get(&session_id)
                    .ok_or_else(|| anyhow!("daemon terminal {session_id} is not running"))?;
                if *active_runtime_id != runtime_id {
                    bail!(
                        "daemon terminal {session_id} belongs to runtime {active_runtime_id}, not {runtime_id}"
                    );
                }
                terminal.resize(cols, rows);
                Ok(ResponsePayload::Ack)
            }
            Command::CloseTerminal => {
                let removed = {
                    let mut terminals = self.terminals.lock();
                    if let Some((active_runtime_id, _)) = terminals.get(&session_id) {
                        if *active_runtime_id != runtime_id {
                            bail!(
                                "daemon terminal {session_id} belongs to runtime {active_runtime_id}, not {runtime_id}"
                            );
                        }
                    }
                    terminals.remove(&session_id)
                };
                drop(removed);
                Ok(ResponsePayload::Ack)
            }
            Command::Start { options } => {
                let previous = self.sessions.lock().remove(&session_id);
                drop(previous);
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
                    subagents: None,
                    provider_cursor: options
                        .provider_cursor
                        .map(serde_json::from_value)
                        .transpose()
                        .context("daemon received an invalid provider cursor")?,
                };
                let handle =
                    self.spawn_runtime(session_id, runtime_id, provider, options, events)?;
                let supports_steer = handle.supports_steer();
                let supports_user_input_actions = handle.supports_user_input_actions();
                self.sessions
                    .lock()
                    .insert(session_id, (runtime_id, handle));
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
                        .is_some_and(|(active_runtime_id, _)| *active_runtime_id == runtime_id)
                        .then(|| sessions.remove(&session_id))
                        .flatten()
                };
                drop(removed);
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
            } => {
                // A scoped credential names its owning session; a master-token
                // request may attribute the prompt to `request.session_id`
                // when it is a known task.
                let sender = agent.or_else(|| {
                    (!session_id.is_nil() && self.known_session(session_id)).then_some(session_id)
                });
                self.agent_create_session(
                    sender,
                    provider,
                    model,
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
            command => {
                // Quarantined transfer sessions hold received files that the
                // user hasn't trusted yet — the composer shows a trust card
                // instead of a prompt field, and the daemon refuses anything
                // that could start the agent on them.
                if matches!(command, Command::Prompt { .. } | Command::Steer { .. })
                    && self.session_quarantined(session_id)
                {
                    bail!("received files are quarantined until trusted");
                }
                let driver = {
                    let sessions = self.sessions.lock();
                    let (active_runtime_id, driver) = sessions
                        .get(&session_id)
                        .ok_or_else(|| anyhow!("daemon session {session_id} is not running"))?;
                    if *active_runtime_id != runtime_id {
                        bail!(
                            "daemon session {session_id} belongs to runtime {active_runtime_id}, not {runtime_id}"
                        );
                    }
                    driver.clone()
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
                    events.send(event_to_wire(DriverEvent::PromptSubmitted {
                        message: prompt.clone(),
                        turn_id: turn_id.unwrap_or_else(Uuid::new_v4),
                        message_id: message_id.unwrap_or_else(Uuid::new_v4),
                        sent_by_task: None,
                        hidden: *hidden,
                    })?)?;
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
    }
}

fn session_projection_precedes(
    existing: &AgentSession,
    incoming: &AgentSession,
    active_runtime_id: Option<Uuid>,
) -> bool {
    let existing_cursor = existing.runtime_event_cursor;
    let incoming_cursor = incoming.runtime_event_cursor;
    if let Some(active_runtime_id) = active_runtime_id {
        let existing_is_active =
            existing_cursor.is_some_and(|cursor| cursor.runtime_id == active_runtime_id);
        let incoming_is_active =
            incoming_cursor.is_some_and(|cursor| cursor.runtime_id == active_runtime_id);
        if existing_is_active != incoming_is_active {
            return existing_is_active;
        }
    }
    match (existing_cursor, incoming_cursor) {
        (Some(existing), Some(incoming))
            if existing.runtime_id == incoming.runtime_id && existing.epoch == incoming.epoch =>
        {
            incoming.sequence < existing.sequence
        }
        (Some(_), None) if existing.status.is_busy() => true,
        _ => incoming.updated_at < existing.updated_at,
    }
}

/// Applies the fields a list projection legitimately carries.
///
/// A skeleton's transcript and cursors are placeholders — only its column
/// values are real, and only while the projection is at least as new as what
/// is stored. `workspace` is a column too, but it is not merged here: a
/// client's copy may predate a move the daemon already recorded, and the
/// stored row stays authoritative either way. `status` is skipped while the
/// daemon owns a live runtime for the session: busy state belongs to that
/// runtime, not to a client's possibly-stale row. Returns whether anything
/// was applied.
fn merge_session_list_columns(
    existing: &mut AgentSession,
    incoming: AgentSession,
    has_active_runtime: bool,
) -> bool {
    if incoming.updated_at < existing.updated_at {
        return false;
    }
    existing.title = incoming.title;
    existing.auto_title = incoming.auto_title;
    existing.project_id = incoming.project_id;
    existing.provider = incoming.provider;
    existing.model = incoming.model;
    if !has_active_runtime {
        existing.status = incoming.status;
    }
    existing.created_at = incoming.created_at;
    existing.updated_at = incoming.updated_at;
    existing.last_reply_at = existing.last_reply_at.max(incoming.last_reply_at);
    existing.archived_at = incoming.archived_at;
    existing.pinned_at = incoming.pinned_at;
    existing.landed_at = incoming.landed_at;
    true
}

fn merge_stale_session_metadata(existing: &mut AgentSession, incoming: AgentSession) {
    if incoming.updated_at >= existing.updated_at {
        existing.title = incoming.title;
        existing.project_id = incoming.project_id;
        existing.provider = incoming.provider;
        existing.model = incoming.model;
        existing.runtime_mode = incoming.runtime_mode;
        existing.reasoning_effort = incoming.reasoning_effort;
        existing.service_tier = incoming.service_tier;
        existing.context_window = incoming.context_window;
        existing.agent_preset = incoming.agent_preset;
        existing.updated_at = incoming.updated_at;
        existing.last_reply_at = incoming.last_reply_at.or(existing.last_reply_at);
        existing.archived_at = incoming.archived_at;
        existing.pinned_at = incoming.pinned_at;
        existing.landed_at = incoming.landed_at;
    }
    for queued in incoming.queued_messages {
        if !existing
            .queued_messages
            .iter()
            .any(|candidate| candidate.id == queued.id)
        {
            existing.queued_messages.push(queued);
        }
    }
}

/// Ending checkpoints are produced and stored by the daemon. A second client
/// may still save a projection created just before capture completed; never
/// let that stale projection erase the canonical Git snapshot.
fn preserve_daemon_checkpoints(existing: &AgentSession, incoming: &mut AgentSession) {
    for turn in &mut incoming.turns {
        let Some(checkpoint) = existing
            .turns
            .iter()
            .find(|candidate| candidate.turn_count == turn.turn_count)
            .and_then(|candidate| candidate.checkpoint.as_ref())
            .filter(|checkpoint| {
                matches!(
                    checkpoint.status,
                    CheckpointStatus::Ready | CheckpointStatus::Unavailable
                )
            })
        else {
            continue;
        };
        turn.checkpoint = Some(checkpoint.clone());
    }
}

impl WakuBackend {
    /// Fork a response using only daemon-host state.
    ///
    /// A browser must never reconstruct or persist this operation itself:
    /// provider-native sessions, checkpoint refs, and the task database all
    /// belong to the daemon and may be on another machine.
    fn fork_session_from_response(
        &self,
        session_id: Uuid,
        turn_count: usize,
    ) -> anyhow::Result<(AgentSession, Option<String>)> {
        let (source, cwd, fork_title) = {
            let mut state = self.task_state.lock();
            let source_index = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("the source task is unavailable"))?;
            self.task_store
                .hydrate(&mut state.sessions[source_index])
                .context("could not load the source task")?;
            let source = state.sessions[source_index].clone();
            let project = state
                .projects
                .iter()
                .find(|project| project.id == source.project_id)
                .ok_or_else(|| anyhow!("the source task project is unavailable"))?;
            let cwd = source.workspace.path().unwrap_or(&project.path).to_owned();
            let fork_title = next_response_fork_title(
                source.display_title(),
                state
                    .sessions
                    .iter()
                    .filter(|session| session.project_id == source.project_id)
                    .map(AgentSession::display_title),
            );
            (source, cwd, fork_title)
        };

        validate_response_fork(&source, turn_count)?;
        let provider_turn_count = source
            .turns
            .iter()
            .take(turn_count)
            .filter(|turn| turn.provider_turn_started)
            .count();
        let turns_to_remove = source.provider_turns_after(turn_count);
        let (provider_cursor, message_ids) = self.fork_provider_response(
            &source,
            &cwd,
            &fork_title,
            turn_count,
            provider_turn_count,
            turns_to_remove,
        )?;
        let mut forked = source
            .fork_through_turn(turn_count, provider_cursor, &fork_title)
            .ok_or_else(|| anyhow!("the selected response cannot be copied"))?;
        if !message_ids.is_empty() {
            for turn in &mut forked.turns {
                if let Some(message_id) = turn.provider_resume_at.as_mut()
                    && let Some(remapped) = message_ids.get(message_id)
                {
                    *message_id = remapped.clone();
                }
            }
        }

        let fork_id = forked.id;
        for turn in &mut forked.turns {
            if let Some(checkpoint) = turn.checkpoint.as_mut() {
                checkpoint.git_ref =
                    crate::checkpoint::checkpoint_ref(fork_id, checkpoint.turn_count);
            }
        }
        let checkpoint_warning =
            crate::checkpoint::copy_session_refs(&cwd, source.id, fork_id, turn_count)
                .err()
                .map(|error| error.to_string());

        let pinned = self.sessions.lock().keys().copied().collect();
        let mut state = self.task_state.lock();
        state.push_session(forked.clone());
        if let Err(error) = self.task_store.save(&mut state) {
            state.sessions.retain(|session| session.id != fork_id);
            let _ = crate::checkpoint::delete_all_session_refs(&cwd, fork_id);
            return Err(error).context("could not save the forked task");
        }
        trim_resident_transcripts(&mut state, &pinned);
        Ok((forked, checkpoint_warning))
    }

    /// Restore the daemon-host worktree, provider conversation, and stored
    /// transcript to immediately before one user turn.
    fn rewind_session_to_message(
        &self,
        session_id: Uuid,
        turn_count: usize,
    ) -> anyhow::Result<(AgentSession, Option<String>)> {
        let (source, cwd) = {
            let mut state = self.task_state.lock();
            let source_index = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("the task is unavailable"))?;
            self.task_store
                .hydrate(&mut state.sessions[source_index])
                .context("could not load the task")?;
            let source = state.sessions[source_index].clone();
            let project = state
                .projects
                .iter()
                .find(|project| project.id == source.project_id)
                .ok_or_else(|| anyhow!("the task project is unavailable"))?;
            let cwd = source.workspace.path().unwrap_or(&project.path).to_owned();
            (source, cwd)
        };
        validate_message_rewind(&source, turn_count)?;

        // Resolve the executable before touching the worktree. Even native
        // transcript operations are immediately followed by a replacement
        // prompt, so accepting a rewind that cannot resume would strand the
        // user at a provider state the UI cannot continue.
        let binary = self.provider_binary(source.provider)?;
        let retained_turn_count = turn_count.saturating_sub(1);
        let previous_turn_count = source.turns.len();
        let rollback_turns = source.provider_turns_after(retained_turn_count);
        let provider_turn_count = source
            .turns
            .iter()
            .take(retained_turn_count)
            .filter(|turn| turn.provider_turn_started)
            .count();
        let provider_resume_at = retained_turn_count
            .checked_sub(1)
            .and_then(|index| source.turns.get(index))
            .and_then(|turn| turn.provider_resume_at.clone());

        let turn_start_ref = crate::checkpoint::turn_start_ref(session_id, turn_count);
        let retained_ref = crate::checkpoint::checkpoint_ref(session_id, retained_turn_count);
        let restore_ref = if crate::checkpoint::has_ref(&cwd, &turn_start_ref) {
            turn_start_ref
        } else {
            retained_ref
        };
        if !crate::checkpoint::has_ref(&cwd, &restore_ref) {
            bail!("the checkpoint before this message is unavailable");
        }

        let safety_ref = format!("refs/waku/revert-backup-{session_id}-{}", Uuid::new_v4());
        crate::checkpoint::capture_ref(&cwd, &safety_ref)
            .context("could not create a rewind safety snapshot")?;
        if let Err(error) = crate::checkpoint::restore_ref(&cwd, &restore_ref) {
            return Err(restore_rewind_safety(
                &cwd,
                &safety_ref,
                "could not restore the selected checkpoint",
                error,
            ));
        }

        let provider_rewind = self.rewind_provider_response(
            &source,
            &cwd,
            &binary,
            retained_turn_count,
            rollback_turns,
            provider_turn_count,
            provider_resume_at,
        );
        let (provider_cursor, message_ids, reset_native_session) = match provider_rewind {
            Ok(result) => result,
            Err(error) => {
                return Err(restore_rewind_safety(
                    &cwd,
                    &safety_ref,
                    "the provider rejected the rewind",
                    error,
                ));
            }
        };

        let _ = crate::checkpoint::delete_ref(&cwd, &safety_ref);
        let cleanup_warning = crate::checkpoint::delete_turn_refs_after(
            &cwd,
            session_id,
            retained_turn_count,
            previous_turn_count,
        )
        .err()
        .map(|error| error.to_string());

        // Every provider resumes from the newly stored cursor on the next
        // prompt. Dropping a resident source driver also prevents its late
        // events from racing the rewound transcript.
        let removed = self.sessions.lock().remove(&session_id);
        drop(removed);

        let mut rewound = source.clone();
        if !message_ids.is_empty() {
            for turn in rewound.turns.iter_mut().take(retained_turn_count) {
                if let Some(remapped) = turn
                    .provider_resume_at
                    .as_ref()
                    .and_then(|message_id| message_ids.get(message_id))
                    .cloned()
                {
                    turn.provider_resume_at = Some(remapped);
                }
            }
        }
        if reset_native_session {
            rewound.provider_cursor = None;
        } else if let Some(cursor) = provider_cursor {
            rewound.provider_cursor = Some(cursor);
        }
        rewound.truncate_after_turn(retained_turn_count);
        rewound.status = SessionStatus::Idle;

        let pinned = self.sessions.lock().keys().copied().collect();
        let mut state = self.task_state.lock();
        let existing = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .ok_or_else(|| anyhow!("the task was removed while it was being rewound"))?;
        *existing = rewound.clone();
        state.mark_session_dirty(session_id);
        self.task_store
            .save(&mut state)
            .context("could not save the rewound task")?;
        trim_resident_transcripts(&mut state, &pinned);
        Ok((rewound, cleanup_warning))
    }

    fn fork_provider_response(
        &self,
        source: &AgentSession,
        cwd: &Path,
        fork_title: &str,
        turn_count: usize,
        provider_turn_count: usize,
        turns_to_remove: usize,
    ) -> anyhow::Result<(ProviderResumeCursor, HashMap<String, String>)> {
        match source.provider {
            ProviderKind::Claude => {
                let Some(ProviderResumeCursor::Claude { session_id, .. }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Claude's native session is unavailable");
                };
                let resume_at = source
                    .turns
                    .get(turn_count.saturating_sub(1))
                    .and_then(|turn| turn.provider_resume_at.clone());
                let fork = fork_provider_session(ProviderSessionForkRequest::Claude {
                    session_id: session_id.clone(),
                    resume_at,
                    turn_count: provider_turn_count,
                    title: fork_title.to_owned(),
                })?;
                Ok((fork.cursor, fork.message_ids))
            }
            ProviderKind::Codex
            | ProviderKind::DeepSeek
            | ProviderKind::Muse
            | ProviderKind::OhMyPi
            | ProviderKind::Pi => Ok((
                self.fork_response_with_driver(source, cwd, turns_to_remove)?,
                HashMap::new(),
            )),
            ProviderKind::Cursor => {
                let fork = fork_provider_session(ProviderSessionForkRequest::Cursor {
                    source: source.clone(),
                    turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::Amp => {
                let Some(ProviderResumeCursor::Amp {
                    thread_id,
                    fork_context,
                }) = source.provider_cursor.as_ref()
                else {
                    bail!("Amp's native thread is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Amp {
                    binary: self.provider_binary(ProviderKind::Amp)?,
                    cwd: cwd.to_owned(),
                    thread_id: thread_id.clone(),
                    fork_context: fork_context.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::OpenCode => {
                let Some(ProviderResumeCursor::OpenCode { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("OpenCode's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::OpenCode {
                    binary: self.provider_binary(ProviderKind::OpenCode)?,
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::OpenCode2 => {
                let Some(ProviderResumeCursor::OpenCode2 { session_id, .. }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("OpenCode 2's native session is unavailable");
                };
                // No cwd: a v2 session carries its own `location`, so there is
                // no server working directory to fork against.
                let fork = fork_provider_session(ProviderSessionForkRequest::OpenCode2 {
                    binary: self.provider_binary(ProviderKind::OpenCode2)?,
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::Grok => {
                let Some(ProviderResumeCursor::Grok { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Grok Build's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Grok {
                    binary: self.provider_binary(ProviderKind::Grok)?,
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::Copilot => {
                let Some(ProviderResumeCursor::Copilot { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("GitHub Copilot's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Copilot {
                    binary: self.provider_binary(ProviderKind::Copilot)?,
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                    title: fork_title.to_owned(),
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            // Unreachable through the UI, which hides branching for providers
            // that answer `supports_conversation_fork` with false.
            ProviderKind::Antigravity
            | ProviderKind::Devin
            | ProviderKind::Droid
            | ProviderKind::Fx
            | ProviderKind::Kimi => {
                bail!(
                    "{} cannot branch a conversation at a turn",
                    source.provider.display_name()
                )
            }
        }
    }

    fn fork_response_with_driver(
        &self,
        source: &AgentSession,
        cwd: &Path,
        turns_to_remove: usize,
    ) -> anyhow::Result<ProviderResumeCursor> {
        if let Some(driver) = self
            .sessions
            .lock()
            .get(&source.id)
            .map(|(_, driver)| driver.clone())
        {
            return driver.fork(turns_to_remove);
        }

        match source.provider {
            ProviderKind::Codex
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Codex { .. })
                ) =>
            {
                bail!("Codex's native thread is unavailable");
            }
            ProviderKind::DeepSeek
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::DeepSeek { .. })
                ) =>
            {
                bail!("DeepSeek Harness's native session is unavailable");
            }
            ProviderKind::Pi
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Pi {
                        session_file: Some(_),
                        ..
                    })
                ) =>
            {
                bail!("Pi's native session file is unavailable");
            }
            ProviderKind::OhMyPi
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::OhMyPi {
                        session_file: Some(_),
                        ..
                    })
                ) =>
            {
                bail!("Oh My Pi's native session file is unavailable");
            }
            ProviderKind::Muse
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Muse { .. })
                ) =>
            {
                bail!("Muse Code's native session is unavailable");
            }
            _ => {}
        }

        let (wake, _wake_events) = smol::channel::bounded(1);
        let (event_sender, _event_receiver) = driver::event_channel(wake);
        let driver = driver::start_local(
            source.provider,
            DriverStartOptions {
                binary: self.provider_binary(source.provider)?,
                cwd: cwd.to_owned(),
                mode: source.runtime_mode,
                model: source.model.clone(),
                reasoning_effort: source.reasoning_effort.clone(),
                service_tier: source.service_tier.clone(),
                context_window: source.context_window.clone(),
                agent_preset: source.agent_preset.clone(),
                computer_use_enabled: false,
                // A fork/rollback driver is a one-shot process, not the
                // task's live runtime; it never receives a scoped token.
                agent: None,
                subagents: None,
                provider_cursor: source.provider_cursor.clone(),
            },
            event_sender,
        )?;
        driver.fork(turns_to_remove)
    }

    #[allow(clippy::too_many_arguments)]
    fn rewind_provider_response(
        &self,
        source: &AgentSession,
        cwd: &Path,
        binary: &Path,
        retained_turn_count: usize,
        rollback_turns: usize,
        provider_turn_count: usize,
        provider_resume_at: Option<String>,
    ) -> anyhow::Result<(Option<ProviderResumeCursor>, HashMap<String, String>, bool)> {
        if rollback_turns == 0 {
            return Ok((None, HashMap::new(), false));
        }
        let reset_native_session = retained_turn_count == 0
            && matches!(
                source.provider,
                ProviderKind::Claude
                    | ProviderKind::Copilot
                    | ProviderKind::Cursor
                    | ProviderKind::Grok
            );
        if reset_native_session {
            return Ok((None, HashMap::new(), true));
        }

        match source.provider {
            ProviderKind::Claude => {
                let Some(ProviderResumeCursor::Claude { session_id, .. }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Claude's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Claude {
                    session_id: session_id.clone(),
                    resume_at: provider_resume_at,
                    turn_count: provider_turn_count,
                    title: format!("{} (rewind)", source.display_title()),
                })?;
                Ok((Some(fork.cursor), fork.message_ids, false))
            }
            ProviderKind::OpenCode => {
                let cursor = if let Some(driver) = self
                    .sessions
                    .lock()
                    .get(&source.id)
                    .map(|(_, driver)| driver.clone())
                {
                    driver
                        .rollback(rollback_turns)?
                        .ok_or_else(|| anyhow!("OpenCode returned no rewound-session cursor"))?
                } else {
                    let Some(ProviderResumeCursor::OpenCode { session_id }) =
                        source.provider_cursor.as_ref()
                    else {
                        bail!("OpenCode's native session is unavailable");
                    };
                    fork_provider_session(ProviderSessionForkRequest::OpenCode {
                        binary: binary.to_owned(),
                        cwd: cwd.to_owned(),
                        session_id: session_id.clone(),
                        turn_count: provider_turn_count,
                    })?
                    .cursor
                };
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::OpenCode2 => {
                let cursor = if let Some(driver) = self
                    .sessions
                    .lock()
                    .get(&source.id)
                    .map(|(_, driver)| driver.clone())
                {
                    driver
                        .rollback(rollback_turns)?
                        .ok_or_else(|| anyhow!("OpenCode 2 returned no rewound-session cursor"))?
                } else {
                    let Some(ProviderResumeCursor::OpenCode2 { session_id, .. }) =
                        source.provider_cursor.as_ref()
                    else {
                        bail!("OpenCode 2's native session is unavailable");
                    };
                    fork_provider_session(ProviderSessionForkRequest::OpenCode2 {
                        binary: binary.to_owned(),
                        session_id: session_id.clone(),
                        turn_count: provider_turn_count,
                    })?
                    .cursor
                };
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Amp => {
                let Some(ProviderResumeCursor::Amp {
                    thread_id,
                    fork_context,
                }) = source.provider_cursor.as_ref()
                else {
                    bail!("Amp's native thread is unavailable");
                };
                let cursor = fork_provider_session(ProviderSessionForkRequest::Amp {
                    binary: binary.to_owned(),
                    cwd: cwd.to_owned(),
                    thread_id: thread_id.clone(),
                    fork_context: fork_context.clone(),
                    turn_count: provider_turn_count,
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Cursor => {
                let cursor = fork_provider_session(ProviderSessionForkRequest::Cursor {
                    source: source.clone(),
                    turn_count: retained_turn_count,
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Grok => {
                let Some(ProviderResumeCursor::Grok { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Grok Build's native session is unavailable");
                };
                let cursor = fork_provider_session(ProviderSessionForkRequest::Grok {
                    binary: binary.to_owned(),
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Copilot => {
                let Some(ProviderResumeCursor::Copilot { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("GitHub Copilot's native session is unavailable");
                };
                let cursor = fork_provider_session(ProviderSessionForkRequest::Copilot {
                    binary: binary.to_owned(),
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                    title: format!("{} (rewind)", source.display_title()),
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Muse => {
                let cursor = if let Some(driver) = self
                    .sessions
                    .lock()
                    .get(&source.id)
                    .map(|(_, driver)| driver.clone())
                {
                    driver
                        .rollback(rollback_turns)?
                        .ok_or_else(|| anyhow!("Muse Code returned no rewound-session cursor"))?
                } else {
                    let Some(ProviderResumeCursor::Muse { session_id, .. }) =
                        source.provider_cursor.as_ref()
                    else {
                        bail!("Muse Code's native session is unavailable");
                    };
                    fork_provider_session(ProviderSessionForkRequest::Muse {
                        binary: binary.to_owned(),
                        session_id: session_id.clone(),
                        turn_count: provider_turn_count,
                    })?
                    .cursor
                };
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Codex
            | ProviderKind::DeepSeek
            | ProviderKind::OhMyPi
            | ProviderKind::Pi => Ok((
                self.rollback_response_with_driver(source, cwd, binary, rollback_turns)?,
                HashMap::new(),
                false,
            )),
            // Unreachable through the UI, which hides rewinding for providers
            // that answer `supports_conversation_rollback` with false.
            ProviderKind::Antigravity
            | ProviderKind::Devin
            | ProviderKind::Droid
            | ProviderKind::Fx
            | ProviderKind::Kimi => {
                bail!(
                    "{} cannot rewind a conversation to a turn",
                    source.provider.display_name()
                )
            }
        }
    }

    fn rollback_response_with_driver(
        &self,
        source: &AgentSession,
        cwd: &Path,
        binary: &Path,
        rollback_turns: usize,
    ) -> anyhow::Result<Option<ProviderResumeCursor>> {
        if let Some(driver) = self
            .sessions
            .lock()
            .get(&source.id)
            .map(|(_, driver)| driver.clone())
        {
            return driver.rollback(rollback_turns);
        }

        let (wake, _wake_events) = smol::channel::bounded(1);
        let (event_sender, _event_receiver) = driver::event_channel(wake);
        let driver = driver::start_local(
            source.provider,
            DriverStartOptions {
                binary: binary.to_owned(),
                cwd: cwd.to_owned(),
                mode: source.runtime_mode,
                model: source.model.clone(),
                reasoning_effort: source.reasoning_effort.clone(),
                service_tier: source.service_tier.clone(),
                context_window: source.context_window.clone(),
                agent_preset: source.agent_preset.clone(),
                computer_use_enabled: false,
                agent: None,
                subagents: None,
                provider_cursor: source.provider_cursor.clone(),
            },
            event_sender,
        )?;
        driver.rollback(rollback_turns)
    }

    fn provider_binary(&self, provider: ProviderKind) -> anyhow::Result<PathBuf> {
        ensure_shell_environment();
        let settings = self.settings.get();
        let binary_override = settings
            .provider_binary_overrides
            .get(&provider)
            .map(String::as_str);
        crate::model::provider_probe(provider, binary_override)
            .path
            .ok_or_else(|| anyhow!("{} is not installed on the daemon", provider.display_name()))
    }

    /// Whether `session_id` names a task the daemon knows.
    fn known_session(&self, session_id: Uuid) -> bool {
        self.task_state
            .lock()
            .sessions
            .iter()
            .any(|session| session.id == session_id)
    }

    /// The whole agent surface sits behind this daemon-level setting. While
    /// it is off the commands are rejected for every caller — scoped
    /// credentials included — and nothing is minted or injected.
    fn require_agent_tools(&self) -> anyhow::Result<()> {
        if !self.settings.get().agent_tools_enabled {
            bail!("agent session commands are disabled on this daemon");
        }
        Ok(())
    }

    /// The settings surface is gated separately from task creation and only
    /// for scoped credentials — a client holding the master token already has
    /// full `updateSettings` access, so the flag must not gate it.
    fn require_agent_settings(&self, agent: Option<Uuid>) -> anyhow::Result<()> {
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
    fn upsert_custom_command(
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

    fn remove_custom_command(
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

    /// Start a provider runtime for `session_id` and forward its events into
    /// `events`, which must already target the new `runtime_id`.
    ///
    /// Shared by client `Start` requests and the agent commands' cold start:
    /// both mint the session's scoped credential (when the daemon's agent
    /// tools are enabled) and both run the forwarder that tracks turns,
    /// drains queued agent prompts, and attributes accepted steers.
    fn spawn_runtime(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        provider: ProviderKind,
        options: DriverStartOptions,
        events: EventSink,
    ) -> anyhow::Result<DriverHandle> {
        let (wake, _wake_events) = smol::channel::bounded(1);
        let (event_sender, event_receiver) = driver::event_channel(wake);
        let mut options = options;
        // The credential exists before the process does so it can travel
        // with the runtime's launch environment. A missing CLI or unset
        // daemon address disables injection for this launch only. Either
        // agent surface — task tools or settings writes — gets it injected.
        let daemon_settings = self.settings.get();
        // Computer Use is experimental: the enable flag only counts while the
        // experiment opt-in is on, whatever a client or a hand-edited settings
        // document sent over the wire.
        options.computer_use_enabled = crate::computer_use::resolve_enabled(
            options.computer_use_enabled,
            daemon_settings.computer_use_experiment_enabled,
        );
        if daemon_settings.agent_tools_enabled || daemon_settings.agent_settings_enabled {
            match self.agent_launch_env(session_id) {
                Ok(launch) => options.agent = Some(launch),
                Err(error) => eprintln!(
                    "goddard-daemon: agent surface unavailable for session {session_id}: {error:#}"
                ),
            }
        }
        // Named subagents ride the launch with the runtime: the built-in
        // explorer plus the configured tiers, priced from the cached rate
        // table (disk only — a session start never waits on the network).
        // Drivers without an injection channel simply ignore it. Still
        // experimental — injected only when the opt-in is on.
        if daemon_settings.subagents_enabled {
            options.subagents = Some(crate::subagents::spec_for(
                provider,
                &daemon_settings.subagent_tiers,
                &crate::usage_history::load_cached_rate_table(&self.usage_rates_dir),
            ));
        }
        // A launch that never came up keeps no credential.
        let handle = match driver::start_local(provider, options, event_sender) {
            Ok(handle) => handle,
            Err(error) => {
                self.agent.revoke_session(session_id);
                return Err(error);
            }
        };
        let forwarder_handle = handle.clone();
        let agent = self.agent.clone();
        let task_state = self.task_state.clone();
        let task_store = self.task_store.clone();
        let sessions = self.sessions.clone();
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
                );
            })
            .context("could not start daemon event forwarding thread")?;
        Ok(handle)
    }

    /// Mint the runtime's scoped credential and assemble the environment the
    /// provider launch receives. The shim directory lives beside the daemon
    /// database so shared-service providers have somewhere private to put a
    /// per-session launcher.
    fn agent_launch_env(&self, session_id: Uuid) -> anyhow::Result<crate::agent::AgentLaunchEnv> {
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
        Ok(crate::agent::AgentLaunchEnv {
            token: self.agent.mint(session_id),
            task_id: session_id,
            daemon_address,
            cli_path,
            shim_directory,
            task_tools: settings.agent_tools_enabled,
            settings_writes: settings.agent_settings_enabled,
        })
    }

    /// Return the live driver for `session_id`, cold-starting it from the
    /// stored task's provider cursor and saved options when no runtime is
    /// running.
    fn ensure_agent_runtime(
        &self,
        session_id: Uuid,
        events: &EventSink,
    ) -> anyhow::Result<(Uuid, DriverHandle)> {
        if let Some((runtime_id, driver)) = self.sessions.lock().get(&session_id) {
            return Ok((*runtime_id, driver.clone()));
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
        if let Some((runtime_id, driver)) = self.sessions.lock().get(&session_id) {
            return Ok((*runtime_id, driver.clone()));
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
                subagents: None,
                provider_cursor: session.provider_cursor.clone(),
            };
            (provider, options)
        };
        let runtime_id = Uuid::new_v4();
        let sink = events.begin_session_runtime(session_id, runtime_id);
        let handle =
            match self.spawn_runtime(session_id, runtime_id, provider, options, sink.clone()) {
                Ok(handle) => handle,
                Err(error) => {
                    sink.end_session_runtime();
                    return Err(error);
                }
            };
        let driver = handle.clone();
        self.sessions
            .lock()
            .insert(session_id, (runtime_id, handle));
        Ok((runtime_id, driver))
    }

    /// `agent create`: build a fully configured task and start its first
    /// prompt. Mirrors the app's New Task defaults — the project must be an
    /// absolute path; an existing project resolves by it, and an unknown
    /// path registers only as a primary checkout.
    fn agent_create_session(
        &self,
        sender: Option<Uuid>,
        provider: ProviderKind,
        model: String,
        project: PathBuf,
        workspace: AgentWorkspace,
        base_branch: Option<String>,
        prompt: String,
        events: EventSink,
    ) -> anyhow::Result<ResponsePayload> {
        self.require_agent_tools()?;
        if prompt.trim().is_empty() {
            bail!("agent sessions require a prompt");
        }
        if !project.is_absolute() {
            bail!("the project path must be absolute");
        }
        let model = match model.trim() {
            "" | "default" => None,
            model => Some(model.to_owned()),
        };
        if matches!(workspace, AgentWorkspace::Worktree)
            && base_branch
                .as_deref()
                .is_none_or(|branch| branch.trim().is_empty())
        {
            bail!("worktree sessions require a base branch");
        }
        let project = std::fs::canonicalize(&project)
            .with_context(|| format!("project path {} does not exist", project.display()))?;
        let (project_id, project_path) = {
            let mut state = self.task_state.lock();
            match state
                .projects
                .iter()
                .find(|existing| {
                    std::fs::canonicalize(&existing.path).is_ok_and(|path| path == project)
                })
                .map(|existing| (existing.id, existing.path.clone()))
            {
                Some(found) => found,
                None => {
                    if crate::worktree::is_linked_worktree(&project) {
                        bail!(
                            "{} is a Git worktree; only primary checkouts can be registered as projects",
                            project.display()
                        );
                    }
                    let registered = Project::from_path(project.clone());
                    let found = (registered.id, registered.path.clone());
                    state.projects.push(registered);
                    found
                }
            }
        };
        let mut session = AgentSession::new(project_id, provider);
        session.model = model;
        session.workspace = match workspace {
            AgentWorkspace::Local => SessionWorkspace::Local,
            AgentWorkspace::Worktree => {
                let created = crate::worktree::create(
                    &project_path,
                    None,
                    base_branch.as_deref(),
                    false,
                    &[],
                )?;
                SessionWorkspace::Worktree {
                    path: created.path,
                    name: created.name,
                    branch: None,
                    base_branch,
                }
            }
        };
        let session_id = session.id;
        let turn_id = Uuid::new_v4();
        let message_id = Uuid::new_v4();
        session.adopt_submitted_prompt(&prompt, turn_id, message_id, sender, false);
        {
            let mut state = self.task_state.lock();
            state.push_session(session);
            self.task_store.save(&mut state)?;
        }
        // The adopted prompt above already persisted, so a launch failure
        // still leaves a normal task behind. Delivering it now starts the
        // first turn immediately.
        let (runtime_id, driver) = self.ensure_agent_runtime(session_id, &events)?;
        let sink = events.for_session(session_id, runtime_id);
        sink.send(event_to_wire(DriverEvent::PromptSubmitted {
            message: prompt.clone(),
            turn_id,
            message_id,
            sent_by_task: sender,
            hidden: false,
        })?)?;
        driver.prompt(prompt);
        Ok(ResponsePayload::AgentSessionCreated { session_id })
    }

    /// Persisted quarantine flag — set on received-file sessions until the
    /// user trusts the transfer. Checked against `task_state`, not the
    /// running-driver map, so it holds for sessions that aren't running.
    fn session_quarantined(&self, session_id: Uuid) -> bool {
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

    /// `agent prompt`: deliver a message to an existing task, by Waku task
    /// id or provider-native thread id. Queue mode holds the prompt in a
    /// daemon-side per-session queue until the target is idle; steer mode
    /// injects it into the running turn.
    fn agent_prompt(
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
        let target = self.resolve_agent_target(task_id, thread_id, provider)?;
        if self.session_quarantined(target) {
            bail!("received files are quarantined until trusted");
        }
        match delivery {
            AgentPromptDelivery::Steer => {
                let driver = self
                    .sessions
                    .lock()
                    .get(&target)
                    .map(|(_, driver)| driver.clone())
                    .ok_or_else(|| anyhow!("task {target} has no running session to steer"))?;
                if !self.agent.has_open_turn(target) {
                    bail!("task {target} has no running turn to steer");
                }
                if !driver.supports_steer() {
                    bail!("the task's provider does not support steering");
                }
                self.agent.record_pending_steer(
                    target,
                    crate::agent::AgentPrompt {
                        prompt: prompt.clone(),
                        sender,
                    },
                );
                driver.steer(prompt);
                Ok(ResponsePayload::Ack)
            }
            AgentPromptDelivery::Queue => {
                // Enqueue before checking turn state so a prompt can never
                // slip between a finishing turn and its queue drain.
                self.agent
                    .enqueue(target, crate::agent::AgentPrompt { prompt, sender });
                if self.agent.is_working(target) {
                    // The runtime event forwarder delivers queued prompts in
                    // order once the provider finishes the turn.
                    return Ok(ResponsePayload::Ack);
                }
                let (runtime_id, driver) = self.ensure_agent_runtime(target, &events)?;
                let sink = events.for_session(target, runtime_id);
                self.drain_agent_queue(target, &driver, &sink)?;
                Ok(ResponsePayload::Ack)
            }
        }
    }

    /// Pop every queued agent prompt for the session, in submission order.
    /// A turn that starts working mid-drain holds the remainder for the
    /// provider's finish event.
    fn drain_agent_queue(
        &self,
        session_id: Uuid,
        driver: &DriverHandle,
        sink: &EventSink,
    ) -> anyhow::Result<()> {
        while let Some(entry) = self.agent.pop_queued(session_id) {
            if self.agent.is_working(session_id) {
                self.agent.requeue_front(session_id, entry);
                break;
            }
            deliver_agent_prompt(
                session_id,
                driver,
                entry,
                sink,
                &self.agent,
                &self.task_state,
                &self.task_store,
            )?;
        }
        Ok(())
    }

    /// Resolve an agent prompt's target: an explicit Waku task id, or a
    /// provider-native Agent CLI thread id matched against every
    /// daemon-known task's stored resume cursor.
    fn resolve_agent_target(
        &self,
        task_id: Option<Uuid>,
        thread_id: Option<String>,
        provider: Option<ProviderKind>,
    ) -> anyhow::Result<Uuid> {
        match (task_id, thread_id) {
            (Some(task_id), None) => self
                .known_session(task_id)
                .then_some(task_id)
                .ok_or_else(|| anyhow!("task {task_id} is unknown to the daemon")),
            (None, Some(thread_id)) => {
                let thread_id = thread_id.trim().to_owned();
                if thread_id.is_empty() {
                    bail!("the thread id must not be empty");
                }
                let mut state = self.task_state.lock();
                let mut matches = Vec::new();
                for index in 0..state.sessions.len() {
                    // The resume cursor lives in the session detail blob, so
                    // skeletons need hydrating before they can answer.
                    self.task_store.hydrate(&mut state.sessions[index])?;
                    let session = &state.sessions[index];
                    let Some(cursor) = &session.provider_cursor else {
                        continue;
                    };
                    if cursor.native_id() == thread_id
                        && provider.is_none_or(|provider| cursor.provider() == provider)
                    {
                        matches.push(session.id);
                    }
                }
                match matches.len() {
                    0 => bail!("no daemon task uses agent thread {thread_id}"),
                    1 => Ok(matches[0]),
                    _ => bail!(
                        "agent thread {thread_id} matches {} tasks; pass provider to disambiguate",
                        matches.len()
                    ),
                }
            }
            _ => bail!("exactly one of task_id and thread_id is required"),
        }
    }
}

fn validate_message_rewind(source: &AgentSession, turn_count: usize) -> anyhow::Result<()> {
    if !matches!(source.status, SessionStatus::Idle | SessionStatus::Failed) {
        bail!("stop the task before editing a prior message");
    }
    let Some(turn) = source
        .turns
        .iter()
        .find(|turn| turn.turn_count == turn_count)
    else {
        bail!("the selected message is unavailable");
    };
    if !source.messages.iter().any(|message| {
        message.turn_id == Some(turn.id) && message.role == crate::model::MessageRole::User
    }) {
        bail!("the selected user message is unavailable");
    }
    let rollback_turns = source.provider_turns_after(turn_count.saturating_sub(1));
    if rollback_turns > 0 && source.provider_cursor.is_none() {
        bail!("the provider conversation is unavailable");
    }
    Ok(())
}

fn restore_rewind_safety(
    cwd: &Path,
    safety_ref: &str,
    context: &str,
    error: anyhow::Error,
) -> anyhow::Error {
    match crate::checkpoint::restore_ref(cwd, safety_ref) {
        Ok(()) => {
            let _ = crate::checkpoint::delete_ref(cwd, safety_ref);
            anyhow!("{context}: {error}; the original worktree was restored")
        }
        Err(restore_error) => anyhow!(
            "{context}: {error}; restoring the safety snapshot also failed: {restore_error}; snapshot: {safety_ref}"
        ),
    }
}

fn validate_response_fork(source: &AgentSession, turn_count: usize) -> anyhow::Result<()> {
    if !matches!(source.status, SessionStatus::Idle | SessionStatus::Failed) {
        bail!("stop the task before forking a response");
    }
    let cursor = source
        .provider_cursor
        .as_ref()
        .ok_or_else(|| anyhow!("the provider conversation is unavailable"))?;
    if cursor.provider() != source.provider {
        bail!("the provider conversation does not match this task");
    }
    if source
        .turns
        .get(turn_count.saturating_sub(1))
        .is_none_or(|turn| turn.turn_count != turn_count || !turn.provider_turn_started)
    {
        bail!("the selected response cannot be forked");
    }
    Ok(())
}

fn numbered_title_suffix(title: &str) -> Option<(&str, usize)> {
    let (base, suffix) = title.rsplit_once(" (")?;
    let number = suffix.strip_suffix(')')?.parse().ok()?;
    (!base.is_empty() && number >= 2).then_some((base, number))
}

fn next_response_fork_title<'a>(
    source_title: &str,
    existing_titles: impl IntoIterator<Item = &'a str>,
) -> String {
    let existing_titles = existing_titles.into_iter().collect::<Vec<_>>();
    let base = numbered_title_suffix(source_title)
        .filter(|(base, _)| existing_titles.iter().any(|title| title == base))
        .map_or(source_title, |(base, _)| base);
    let highest_number = existing_titles
        .iter()
        .filter_map(|title| {
            if *title == base {
                Some(1)
            } else {
                numbered_title_suffix(title)
                    .filter(|(candidate_base, _)| *candidate_base == base)
                    .map(|(_, number)| number)
            }
        })
        .max()
        .unwrap_or(1);
    format!("{base} ({})", highest_number.saturating_add(1).max(2))
}

fn fork_provider_session(
    request: ProviderSessionForkRequest,
) -> anyhow::Result<ProviderSessionFork> {
    use crate::model::ProviderResumeCursor;

    let (cursor, message_ids, source_resume_at) = match request {
        ProviderSessionForkRequest::Claude {
            session_id,
            resume_at,
            turn_count,
            title,
        } => {
            let source_resume_at = resume_at.map(Ok).unwrap_or_else(|| {
                crate::claude_session::message_id_for_turn(&session_id, turn_count)
            })?;
            let fork =
                crate::claude_session::fork_session_at(&session_id, &source_resume_at, &title)?;
            let fork_resume_at = fork
                .message_ids
                .get(&source_resume_at)
                .cloned()
                .ok_or_else(|| anyhow!("Claude fork did not include its target message"))?;
            (
                ProviderResumeCursor::Claude {
                    session_id: fork.session_id,
                    resume_at: Some(fork_resume_at),
                },
                fork.message_ids,
                Some(source_resume_at),
            )
        }
        ProviderSessionForkRequest::Amp {
            binary,
            cwd,
            thread_id,
            fork_context,
            turn_count,
        } => (
            crate::amp_session::fork_session_at_turn(
                &binary,
                &cwd,
                &thread_id,
                fork_context.as_deref(),
                turn_count,
            )?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::Cursor { source, turn_count } => (
            crate::cursor_session::fork_session_at_turn(&source, turn_count)?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::OpenCode {
            binary,
            cwd,
            session_id,
            turn_count,
        } => (
            crate::opencode_session::fork_session_at_turn(&binary, &cwd, &session_id, turn_count)?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::OpenCode2 {
            binary,
            session_id,
            turn_count,
        } => (
            crate::opencode2_session::fork_session_at_turn(&binary, &session_id, turn_count)?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::Grok {
            binary,
            cwd,
            session_id,
            turn_count,
        } => (
            crate::grok_session::fork_session_at_turn(&binary, &cwd, &session_id, turn_count)?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::Copilot {
            binary,
            cwd,
            session_id,
            turn_count,
            title,
        } => (
            crate::copilot_session::fork_session_at_turn(
                &binary, &cwd, &session_id, turn_count, &title,
            )?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::Muse {
            binary,
            session_id,
            turn_count,
        } => (
            crate::muse_session::fork_session_at_turn(&binary, &session_id, turn_count)?,
            HashMap::new(),
            None,
        ),
    };
    Ok(ProviderSessionFork {
        cursor,
        message_ids,
        source_resume_at,
    })
}

fn handle_driver_command(
    driver: &DriverHandle,
    command: Command,
) -> anyhow::Result<ResponsePayload> {
    match command {
        Command::Prompt {
            prompt, attachments, ..
        } => driver.prompt_with_attachments(prompt, attachments),
        Command::Steer { prompt } => driver.steer(prompt),
        Command::ClarifyUserInput {
            request_id,
            content,
        } => driver.clarify_user_input(request_id, content),
        Command::CancelUserInput { request_id } => driver.cancel_user_input(request_id),
        Command::Cancel => driver.cancel(),
        Command::CancelComputerUse => driver.cancel_computer_use(),
        Command::RefreshBackgroundWork => driver.refresh_background_work(),
        Command::StopBackgroundWork { key, control_id } => {
            driver.stop_background_work(
                serde_json::from_value(key).context("invalid background-work key")?,
                control_id,
            );
        }
        Command::Respond {
            request_id,
            option_id,
        } => driver.respond(request_id, option_id),
        Command::RespondUserInput {
            request_id,
            answers,
        } => driver.respond_user_input(request_id, answers),
        Command::Goal { operation } => driver.goal(operation),
        // Fire-and-forget like Goal: admission and the outcome arrive as
        // driver events, so the caller never waits on a response.
        Command::Compact => driver.compact(),
        Command::RunComputerTool { request } => {
            driver.run_computer_tool(crate::computer_use::ComputerToolRequest {
                call_id: request.call_id,
                tool: request.tool,
                arguments: request.arguments,
            });
        }
        Command::RejectComputerTool { request, reason } => {
            driver.reject_computer_tool(
                crate::computer_use::ComputerToolRequest {
                    call_id: request.call_id,
                    tool: request.tool,
                    arguments: request.arguments,
                },
                reason,
            );
        }
        Command::ApplyOptions { options } => {
            return Ok(ResponsePayload::OptionsApplied {
                applied: driver.apply_options(SessionOptions {
                    mode: decode_enum(&options.mode)?,
                    model: options.model,
                    reasoning_effort: options.reasoning_effort,
                    service_tier: options.service_tier,
                    context_window: options.context_window,
                }),
            });
        }
        Command::Rollback { turns } => {
            let cursor = driver
                .rollback(turns)?
                .map(serde_json::to_value)
                .transpose()?;
            return Ok(ResponsePayload::Cursor { cursor });
        }
        Command::Fork { turns_to_remove } => {
            let cursor = Some(serde_json::to_value(driver.fork(turns_to_remove)?)?);
            return Ok(ResponsePayload::Cursor { cursor });
        }
        Command::AttachSession
        | Command::Start { .. }
        | Command::GetSettings
        | Command::UpdateSettings { .. }
        | Command::ProbeProvider { .. }
        | Command::FetchPlanUsage { .. }
        | Command::ProbeComputerPermissions { .. }
        | Command::Evaluate { .. }
        | Command::RouteTask { .. }
        | Command::RecordRouteOverride { .. }
        | Command::GetRoutePolicy
        | Command::SetRouteClassTarget { .. }
        | Command::LoadUsageHistory { .. }
        | Command::LoadSkills { .. }
        | Command::SetSkillsEnabled { .. }
        | Command::TrashSkills { .. }
        | Command::LoadTaskState
        | Command::SaveTaskState { .. }
        | Command::RemoveSession
        | Command::HydrateSession { .. }
        | Command::SearchSessionMessages { .. }
        | Command::ListProviderSessions { .. }
        | Command::LoadProviderSession { .. }
        | Command::LoadComposerDrafts
        | Command::SaveComposerDrafts { .. }
        | Command::ApplyComposerDraftChanges { .. }
        | Command::StoreBlob { .. }
        | Command::ImportAttachment { .. }
        | Command::ImportPathAttachment { .. }
        | Command::ReadBlob { .. }
        | Command::ReadAttachment { .. }
        | Command::SweepBlobs
        | Command::ForkSessionFromResponse { .. }
        | Command::RewindSessionToMessage { .. }
        | Command::ForkProviderSession { .. }
        | Command::Workspace { .. }
        | Command::OpenTerminal { .. }
        | Command::WriteTerminal { .. }
        | Command::ResizeTerminal { .. }
        | Command::CloseTerminal
        | Command::CloseSession
        | Command::AgentCreateSession { .. }
        | Command::AgentPrompt { .. }
        | Command::UpsertCustomCommand { .. }
        | Command::RemoveCustomCommand { .. }
        | Command::ListCustomCommands
        | Command::GetFriends
        | Command::SendFriendRequest { .. }
        | Command::RespondFriendRequest { .. }
        | Command::WithdrawFriendRequest { .. }
        | Command::RemoveFriend { .. }
        | Command::SendFileToFriend { .. }
        | Command::CancelTransfer { .. }
        | Command::ProbeFriend { .. } => {
            bail!("daemon received a command in the wrong dispatch path")
        }
    }
    Ok(ResponsePayload::Ack)
}

fn ensure_shell_environment() {
    static REFRESHED: OnceLock<()> = OnceLock::new();
    REFRESHED.get_or_init(|| {
        crate::command_env::refresh_from_default_shell();
    });
}

/// Pump provider events for one runtime into the client's event stream.
///
/// Besides serialization this maintains the agent-surface bookkeeping: turn
/// state decides where queue-mode prompts wait, a finished turn drains that
/// queue in submission order, a `steerAccepted` echo is annotated with the
/// sending task, and a dead runtime drops its credential and registry entry
/// with it.
fn forward_driver_events(
    session_id: Uuid,
    runtime_id: Uuid,
    event_receiver: crossbeam_channel::Receiver<DriverEvent>,
    events: EventSink,
    driver: DriverHandle,
    agent: Arc<crate::agent::AgentState>,
    task_state: Arc<Mutex<PersistedState>>,
    task_store: Arc<StateStore>,
    sessions: Arc<Mutex<HashMap<Uuid, (Uuid, DriverHandle)>>>,
) {
    while let Ok(event) = event_receiver.recv() {
        agent.note_driver_event(session_id, &event);
        let event = match event {
            DriverEvent::Connected { provider_cursor } => {
                // The daemon keeps its own copy of the resume cursor so
                // thread-id resolution and cold starts work even when no
                // client ever saves the task.
                record_provider_cursor(&task_state, &task_store, session_id, &provider_cursor);
                DriverEvent::Connected { provider_cursor }
            }
            DriverEvent::SteerAccepted { message, .. } => {
                let sent_by_task = agent
                    .take_pending_steer(session_id, &message)
                    .and_then(|steer| steer.sender);
                if let Some(sender) = sent_by_task {
                    record_agent_steer(&task_state, &task_store, session_id, &message, sender);
                }
                DriverEvent::SteerAccepted {
                    message,
                    sent_by_task,
                }
            }
            event => event,
        };
        // A finished turn frees the session for the next queued prompt; a
        // `connected` greeting means a freshly (re)started runtime is idle,
        // so prompts queued while it was down deliver now.
        let drains_queue = matches!(
            &event,
            DriverEvent::TurnFinished { .. } | DriverEvent::Connected { .. }
        );
        let process_exited = matches!(&event, DriverEvent::ProcessExited);
        let wire = event_to_wire(event).unwrap_or_else(|error| {
            WireDriverEvent::new(
                "error",
                Value::String(format!("could not encode daemon event: {error}")),
            )
        });
        if events.send(wire).is_err() {
            break;
        }
        if drains_queue {
            while let Some(entry) = agent.pop_queued(session_id) {
                if agent.is_working(session_id) {
                    // A turn started while the queue drained — a human
                    // prompt, or a provider-side wake. Queue-mode messages
                    // wait for the finish rather than steer mid-turn.
                    agent.requeue_front(session_id, entry);
                    break;
                }
                if let Err(error) = deliver_agent_prompt(
                    session_id,
                    &driver,
                    entry,
                    &events,
                    &agent,
                    &task_state,
                    &task_store,
                ) {
                    eprintln!(
                        "goddard-daemon could not deliver a queued agent prompt for task {session_id}: {error:#}"
                    );
                    break;
                }
            }
        }
        if process_exited {
            agent.clear_session(session_id);
            let mut sessions = sessions.lock();
            if sessions
                .get(&session_id)
                .is_some_and(|(active_runtime_id, _)| *active_runtime_id == runtime_id)
            {
                sessions.remove(&session_id);
            }
            break;
        }
    }
}

/// Deliver one queued agent prompt to a live session. A session with an
/// open but parked turn is messaged through the provider's steer path so
/// the prompt folds into the waiting turn; anything else begins a normal
/// new turn whose `promptSubmitted` broadcast carries the sender's
/// provenance.
fn deliver_agent_prompt(
    session_id: Uuid,
    driver: &DriverHandle,
    entry: crate::agent::AgentPrompt,
    sink: &EventSink,
    agent: &crate::agent::AgentState,
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
) -> anyhow::Result<()> {
    if agent.has_parked_turn(session_id) && driver.supports_steer() {
        let prompt = entry.prompt.clone();
        agent.record_pending_steer(session_id, entry);
        driver.steer(prompt);
        return Ok(());
    }
    let turn_id = Uuid::new_v4();
    let message_id = Uuid::new_v4();
    persist_agent_prompt(
        task_state,
        task_store,
        session_id,
        &entry.prompt,
        turn_id,
        message_id,
        entry.sender,
    )?;
    sink.send(event_to_wire(DriverEvent::PromptSubmitted {
        message: entry.prompt.clone(),
        turn_id,
        message_id,
        sent_by_task: entry.sender,
        hidden: false,
    })?)?;
    driver.prompt(entry.prompt);
    Ok(())
}

/// Mirror an accepted agent prompt into the daemon's stored copy of the
/// task, so the message and its sender provenance persist even when no
/// client is attached to adopt it.
fn persist_agent_prompt(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    message: &str,
    turn_id: Uuid,
    message_id: Uuid,
    sent_by_task: Option<Uuid>,
) -> anyhow::Result<()> {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        bail!("task {session_id} is unknown to the daemon");
    };
    task_store.hydrate(session)?;
    if session.adopt_submitted_prompt(message, turn_id, message_id, sent_by_task, false) {
        state.mark_session_dirty(session_id);
        task_store.save(&mut state)?;
    }
    Ok(())
}

/// Mirror a provider-accepted agent steer into the stored task the way
/// [`persist_agent_prompt`] mirrors a queued prompt.
fn record_agent_steer(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    message: &str,
    sent_by_task: Uuid,
) {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return;
    };
    if task_store.hydrate(session).is_err() {
        return;
    }
    session.push_user_message_with_presentation(message, None, Vec::new(), Some(sent_by_task));
    state.mark_session_dirty(session_id);
    if let Err(error) = task_store.save(&mut state) {
        eprintln!(
            "goddard-daemon could not persist an agent steer for task {session_id}: {error:#}"
        );
    }
}

/// Keep the daemon's stored copy of a task's provider cursor current so
/// cold starts and thread-id resolution work without a client ever saving.
fn record_provider_cursor(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    provider_cursor: &Option<ProviderResumeCursor>,
) {
    let Some(cursor) = provider_cursor else {
        return;
    };
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return;
    };
    if task_store.hydrate(session).is_err() || session.provider_cursor.as_ref() == Some(cursor) {
        return;
    }
    session.provider_cursor = Some(cursor.clone());
    state.mark_session_dirty(session_id);
    if let Err(error) = task_store.save(&mut state) {
        eprintln!(
            "goddard-daemon could not persist a provider cursor for task {session_id}: {error:#}"
        );
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_runtime_projection_keeps_newer_transcript_cursor() {
        let runtime_id = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let mut existing = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        existing.status = SessionStatus::Working;
        existing.runtime_event_cursor = Some(crate::model::RuntimeEventCursor {
            runtime_id,
            epoch,
            sequence: 10,
        });
        existing.push_message(crate::model::MessageRole::Assistant, "complete so far");

        let mut stale = existing.clone();
        stale.title = "Renamed elsewhere".into();
        stale.messages.clear();
        stale.runtime_event_cursor = Some(crate::model::RuntimeEventCursor {
            runtime_id,
            epoch,
            sequence: 7,
        });

        assert!(session_projection_precedes(
            &existing,
            &stale,
            Some(runtime_id)
        ));
        merge_stale_session_metadata(&mut existing, stale);
        assert_eq!(existing.title, "Renamed elsewhere");
        assert_eq!(existing.messages.len(), 1);
        assert_eq!(existing.runtime_event_cursor.unwrap().sequence, 10);
    }

    #[test]
    fn client_projection_cannot_replace_a_daemon_checkpoint() {
        let mut existing = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        existing.begin_turn("change it");
        existing.finish_active_turn(crate::model::TurnStatus::Completed);
        let checkpoint = Checkpoint {
            turn_count: 1,
            git_ref: "refs/waku/canonical".into(),
            status: CheckpointStatus::Ready,
            files: Vec::new(),
            additions: 0,
            deletions: 0,
            created_at: 1,
        };
        existing.turns[0].checkpoint = Some(checkpoint.clone());

        let mut incoming = existing.clone();
        incoming.turns[0].checkpoint = Some(Checkpoint {
            git_ref: "refs/waku/stale-client".into(),
            ..checkpoint.clone()
        });
        preserve_daemon_checkpoints(&existing, &mut incoming);

        assert_eq!(incoming.turns[0].checkpoint.as_ref(), Some(&checkpoint));
    }

    #[test]
    fn expired_archives_are_purged_when_state_loads() {
        let root = std::env::temp_dir().join(format!("waku-archive-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        let expired_id = state.sessions[0].id;
        state.sessions[0].begin_turn("expired archive");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state.sessions[0].archived_at =
            Some(crate::model::unix_time() - ARCHIVED_SESSION_RETENTION_SECONDS - 1);

        let project_id = state.projects[0].id;
        let mut recent = AgentSession::new(project_id, ProviderKind::Codex);
        recent.begin_turn("recent archive");
        recent.finish_active_turn(crate::model::TurnStatus::Completed);
        recent.archived_at = Some(crate::model::unix_time());
        let recent_id = recent.id;
        state.push_session(recent);

        let mut active = AgentSession::new(project_id, ProviderKind::Codex);
        active.begin_turn("still active");
        active.finish_active_turn(crate::model::TurnStatus::Completed);
        let active_id = active.id;
        state.push_session(active);
        store.save(&mut state).unwrap();

        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap();
        let remaining = backend.task_state.lock().sessions.clone();
        assert!(
            !remaining.iter().any(|session| session.id == expired_id),
            "an archive past the retention window is removed on load"
        );
        assert!(
            remaining
                .iter()
                .any(|session| session.id == recent_id && session.archived_at.is_some()),
            "a recent archive survives the sweep and stays archived"
        );
        assert!(remaining.iter().any(|session| session.id == active_id));

        // The row is gone from storage too, so it cannot come back.
        let reloaded = StateStore::daemon(root.join("app.db")).load().unwrap();
        assert!(
            !reloaded
                .sessions
                .iter()
                .any(|session| session.id == expired_id)
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn incoming_transfer_materializes_a_quarantined_session() {
        let root = std::env::temp_dir().join(format!("waku-transfer-{}", Uuid::new_v4()));
        let share_dir = root.join("share");
        let store = Arc::new(StateStore::daemon(root.join("app.db")));
        let task_state = Arc::new(Mutex::new(PersistedState::fresh(root.join("repo"))));
        let transfer = waku_protocol::friends::TransferInfo {
            id: Uuid::new_v4(),
            direction: waku_protocol::friends::TransferDirection::Incoming,
            peer_id: "peer".into(),
            peer_name: "maya".into(),
            title: "design.pdf".into(),
            note: Some("here's the new mockups".into()),
            status: waku_protocol::friends::TransferStatus::Done,
            bytes_done: 12,
            bytes_total: 12,
            dest_dir: Some(share_dir.join("transfers/x")),
            session_id: None,
        };

        let session_id =
            create_transfer_session(&task_state, &store, &share_dir, &transfer).unwrap();

        {
            let state = task_state.lock();
            let project = state
                .projects
                .iter()
                .find(|project| project.path == share_dir)
                .expect("a Friends project at the share dir");
            assert_eq!(project.name, "Friends");
            let session = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .expect("the transfer's session");
            assert!(session.quarantined, "received files start untrusted");
            assert_eq!(session.status, SessionStatus::Idle);
            assert_eq!(session.project_id, project.id);
            assert_eq!(session.title, "design.pdf from maya");
            let receipt = &session.turns[0];
            assert_eq!(receipt.status, crate::model::TurnStatus::Completed);
            let receipt_text = session
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(receipt_text.contains("here's the new mockups"));
            assert!(receipt_text.contains("transfers/x"));
        }

        // A second transfer reuses the same Friends project.
        let second = waku_protocol::friends::TransferInfo {
            id: Uuid::new_v4(),
            dest_dir: Some(share_dir.join("transfers/y")),
            ..transfer.clone()
        };
        create_transfer_session(&task_state, &store, &share_dir, &second).unwrap();
        assert_eq!(
            task_state
                .lock()
                .projects
                .iter()
                .filter(|project| project.path == share_dir)
                .count(),
            1
        );

        // The flag survives a reload — quarantine isn't a runtime accident.
        // It lives in the session detail, so the list skeleton reads false
        // and hydrate restores the persisted value.
        let reload_store = StateStore::daemon(root.join("app.db"));
        let mut reloaded = reload_store.load().unwrap();
        let index = reloaded
            .sessions
            .iter()
            .position(|session| session.id == session_id)
            .unwrap();
        assert!(!reloaded.sessions[index].quarantined);
        reload_store
            .hydrate(&mut reloaded.sessions[index])
            .unwrap();
        assert!(reloaded.sessions[index].quarantined);

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn response_fork_titles_follow_one_numbered_sequence() {
        assert_eq!(
            next_response_fork_title("Fix the bug", ["Fix the bug"]),
            "Fix the bug (2)"
        );
        assert_eq!(
            next_response_fork_title(
                "Fix the bug (2)",
                ["Fix the bug", "Fix the bug (2)", "Fix the bug (4)"]
            ),
            "Fix the bug (5)"
        );
        assert_eq!(
            next_response_fork_title("Plan (2026)", ["Plan (2026)"]),
            "Plan (2026) (2)"
        );
    }

    #[test]
    fn message_rewind_requires_a_settled_user_turn_and_provider_cursor() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("change it");
        session.mark_active_turn_provider_started();
        session.provider_cursor = Some(ProviderResumeCursor::Codex {
            thread_id: "thread".into(),
        });
        session.finish_active_turn(crate::model::TurnStatus::Completed);

        assert!(validate_message_rewind(&session, 1).is_ok());

        let mut busy = session.clone();
        busy.status = SessionStatus::Working;
        assert!(validate_message_rewind(&busy, 1).is_err());

        let mut missing_cursor = session.clone();
        missing_cursor.provider_cursor = None;
        assert!(validate_message_rewind(&missing_cursor, 1).is_err());

        let mut missing_message = session;
        missing_message.messages.clear();
        assert!(validate_message_rewind(&missing_message, 1).is_err());
    }

    #[test]
    fn wire_event_round_trip_preserves_ordered_delta_payload() {
        let wire = event_to_wire(DriverEvent::TextDelta("hello".into())).unwrap();
        assert_eq!(wire.kind, "textDelta");
        assert!(matches!(
            event_from_wire(wire).unwrap(),
            DriverEvent::TextDelta(text) if text == "hello"
        ));
    }

    #[test]
    fn wire_event_round_trip_preserves_prompt_submission_identity() {
        let turn_id = Uuid::new_v4();
        let message_id = Uuid::new_v4();
        let wire = event_to_wire(DriverEvent::PromptSubmitted {
            message: "ship it".into(),
            turn_id,
            message_id,
            sent_by_task: None,
            hidden: false,
        })
        .unwrap();
        assert_eq!(wire.kind, "promptSubmitted");
        assert_eq!(wire.payload["message"], "ship it");
        assert_eq!(wire.payload["turnId"], turn_id.to_string());
        assert_eq!(wire.payload["messageId"], message_id.to_string());
        assert!(matches!(
            event_from_wire(wire).unwrap(),
            DriverEvent::PromptSubmitted { message, turn_id: decoded_turn, message_id: decoded_message, .. }
                if message == "ship it" && decoded_turn == turn_id && decoded_message == message_id
        ));
    }

    #[test]
    fn wire_event_round_trip_preserves_agent_provenance() {
        let sender = Uuid::new_v4();
        let wire = event_to_wire(DriverEvent::PromptSubmitted {
            message: "from another task".into(),
            turn_id: Uuid::new_v4(),
            message_id: Uuid::new_v4(),
            sent_by_task: Some(sender),
            hidden: false,
        })
        .unwrap();
        assert_eq!(wire.payload["sentByTask"], sender.to_string());
        assert!(matches!(
            event_from_wire(wire).unwrap(),
            DriverEvent::PromptSubmitted { sent_by_task: Some(decoded), .. } if decoded == sender
        ));

        // An older daemon's payload lacks the field and still decodes.
        let wire = WireDriverEvent::new(
            "promptSubmitted",
            json!({
                "message": "old",
                "turnId": Uuid::new_v4(),
                "messageId": Uuid::new_v4(),
            }),
        );
        assert!(matches!(
            event_from_wire(wire).unwrap(),
            DriverEvent::PromptSubmitted {
                sent_by_task: None,
                ..
            }
        ));
        let wire = WireDriverEvent::new("steerAccepted", json!({ "message": "old" }));
        assert!(matches!(
            event_from_wire(wire).unwrap(),
            DriverEvent::SteerAccepted {
                sent_by_task: None,
                ..
            }
        ));
    }
}
