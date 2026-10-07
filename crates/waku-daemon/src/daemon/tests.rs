use super::*;
use crate::model::{ActivityItem, MessageRole, TranscriptBlock};

fn detailed_session() -> AgentSession {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("Ask");
    session.push_message(MessageRole::Assistant, "an answer");
    session
}

#[test]
fn employee_archive_guard_preserves_dirty_and_unlanded_work() {
    let root = std::env::temp_dir().join(format!("archive-guard-{}", Uuid::new_v4()));
    let repository = root.join("repository");
    let worktree = root.join("worktree");
    std::fs::create_dir_all(&repository).unwrap();
    let git = |cwd: &Path, args: &[&str]| {
        let output = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&repository, &["init", "--quiet", "-b", "main"]);
    git(&repository, &["config", "user.name", "Goddard Tests"]);
    git(&repository, &["config", "user.email", "waku@example.com"]);
    std::fs::write(repository.join("README.md"), "base\n").unwrap();
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "--quiet", "-m", "initial"]);
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );

    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.boss_managed = true;
    session.workspace = SessionWorkspace::Worktree {
        path: worktree.clone(),
        name: "employee".into(),
        branch: None,
        base_branch: Some("main".into()),
        adopted_by: None,
    };
    assert_eq!(
        WakuBackend::employee_archive_blocker(&session).unwrap(),
        None
    );

    std::fs::write(worktree.join("scratch.txt"), "unfinished\n").unwrap();
    assert!(matches!(
        WakuBackend::employee_archive_blocker(&session).unwrap(),
        Some((path, "uncommitted work")) if path == worktree
    ));
    std::fs::remove_file(worktree.join("scratch.txt")).unwrap();

    std::fs::write(worktree.join("land-me.txt"), "commit\n").unwrap();
    git(&worktree, &["add", "."]);
    git(&worktree, &["commit", "--quiet", "-m", "employee work"]);
    assert!(matches!(
        WakuBackend::employee_archive_blocker(&session).unwrap(),
        Some((path, "unlanded commits")) if path == worktree
    ));
    std::fs::remove_dir_all(root).ok();
}

/// The wire session a client sends after `baseline`: full scalars, but
/// `messages`/`transcript_blocks` carry only the appended tail.
fn tail_wire(base: &AgentSession, full: &AgentSession) -> (AgentSession, SessionDetailTail) {
    let mut wire = full.clone();
    wire.messages = wire.messages.split_off(base.messages.len());
    wire.transcript_blocks = wire
        .transcript_blocks
        .split_off(base.transcript_blocks.len());
    let tail = SessionDetailTail {
        session_id: base.id,
        messages_from: base.messages.len() as u32,
        transcript_blocks_from: base.transcript_blocks.len() as u32,
        prefix_signature: detail_prefix_signature(&base.messages, &base.transcript_blocks),
    };
    (wire, tail)
}

#[test]
fn tail_save_splices_onto_the_resident_prefix() {
    let baseline = detailed_session();
    let mut full = baseline.clone();
    full.push_message(MessageRole::Assistant, "a follow-up");
    let (mut wire, tail) = tail_wire(&baseline, &full);

    let mut bases = HashMap::new();
    splice_session_tail(&[baseline], &mut bases, &mut wire, tail);

    assert_eq!(wire.messages.len(), full.messages.len());
    assert_eq!(
        wire.messages.last().unwrap().content,
        full.messages.last().unwrap().content
    );
    assert_eq!(
        wire.messages[..2]
            .iter()
            .map(|m| m.content.clone())
            .collect::<Vec<_>>(),
        vec!["Ask".to_owned(), "an answer".to_owned()]
    );
}

#[test]
fn tail_save_with_a_diverged_resident_prefix_merges_as_a_skeleton() {
    let baseline = detailed_session();
    let mut resident = baseline.clone();
    // Another writer landed first — the resident prefix no longer
    // matches the client's claimed one.
    resident.messages[1].content = "someone else's answer".to_owned();
    let mut full = baseline.clone();
    full.push_message(MessageRole::Assistant, "a follow-up");
    let (mut wire, tail) = tail_wire(&baseline, &full);

    let mut bases = HashMap::new();
    splice_session_tail(&[resident], &mut bases, &mut wire, tail);

    assert!(!wire.detail_loaded);
    assert!(wire.messages.is_empty());
    assert!(wire.transcript_blocks.is_empty());
}

#[test]
fn tail_save_splices_onto_a_stored_row_when_nothing_is_resident() {
    let baseline = detailed_session();
    let mut full = baseline.clone();
    full.push_message(MessageRole::Assistant, "a follow-up");
    let (mut wire, tail) = tail_wire(&baseline, &full);

    let mut bases = HashMap::from([(baseline.id, baseline)]);
    splice_session_tail(&[], &mut bases, &mut wire, tail);

    assert_eq!(wire.messages.len(), full.messages.len());
    assert_eq!(wire.messages.last().unwrap().content, "a follow-up");
}

#[test]
fn tail_save_without_any_prefix_merges_as_a_skeleton() {
    let baseline = detailed_session();
    let mut full = baseline.clone();
    full.push_message(MessageRole::Assistant, "a follow-up");
    let (mut wire, tail) = tail_wire(&baseline, &full);

    // Neither resident nor stored — the tail has nothing to land on.
    let mut bases = HashMap::new();
    splice_session_tail(&[], &mut bases, &mut wire, tail);

    assert!(!wire.detail_loaded);
    assert!(wire.messages.is_empty());
}

#[test]
fn a_zero_prefix_tail_is_a_complete_save() {
    let session = detailed_session();
    let mut wire = session.clone();
    let tail = SessionDetailTail {
        session_id: session.id,
        messages_from: 0,
        transcript_blocks_from: 0,
        prefix_signature: 0,
    };
    let mut bases = HashMap::new();
    splice_session_tail(&[], &mut bases, &mut wire, tail);

    assert!(wire.detail_loaded);
    assert_eq!(wire.messages.len(), session.messages.len());
}

#[test]
fn a_save_cannot_resurrect_pruned_archive_payloads() {
    let mut existing = detailed_session();
    existing.archived_at = Some(1);
    existing.transcript_blocks.push(TranscriptBlock {
        after_message: 1,
        turn_id: None,
        activities: vec![ActivityItem::new(
            None,
            crate::model::ActivityKind::Tool,
            "Ran tests",
            None,
            true,
        )],
    });
    existing.details_pruned = true;

    // The client hydrated before the sweep: its copy still carries the
    // stripped payloads.
    let mut incoming = existing.clone();
    incoming.details_pruned = false;
    incoming.transcript_blocks[0].activities[0].output = Some("big output".into());

    honor_details_pruned(&existing, &mut incoming);
    assert!(incoming.details_pruned);
    assert!(incoming.transcript_blocks[0].activities[0].output.is_none());

    // Unarchiving clears the marker so new work keeps its payloads.
    let mut unarchived = existing.clone();
    unarchived.archived_at = None;
    unarchived.transcript_blocks[0].activities[0].output = Some("fresh".into());
    honor_details_pruned(&existing, &mut unarchived);
    assert!(!unarchived.details_pruned);
    assert_eq!(
        unarchived.transcript_blocks[0].activities[0]
            .output
            .as_deref(),
        Some("fresh")
    );
}

#[test]
fn managed_goal_turn_has_one_claimant_across_clients() {
    let mut claims = HashMap::new();
    let session = Uuid::new_v4();
    let goal = Uuid::new_v4();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    assert!(claim_managed_goal_turn(&mut claims, session, goal, first));
    assert!(!claim_managed_goal_turn(&mut claims, session, goal, first));
    assert!(claim_managed_goal_turn(&mut claims, session, goal, second));
    assert!(!claim_managed_goal_turn(&mut claims, session, goal, first));
    assert!(claim_managed_goal_turn(
        &mut claims,
        session,
        Uuid::new_v4(),
        first
    ));
    assert!(!claim_managed_goal_turn(&mut claims, session, goal, first));
}

/// The auth-status command reads the per-provider shared home — a
/// credential file under `sandbox-homes/<id>` flips the answer.
#[test]
fn sandbox_auth_status_reflects_the_provider_home() {
    let root = std::env::temp_dir().join(format!("waku-sandbox-auth-{}", Uuid::new_v4()));
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    let request = |command| waku_protocol::Request {
        request_id: Uuid::new_v4(),
        session_id: Uuid::nil(),
        runtime_id: Uuid::nil(),
        command,
    };

    let status = backend
        .handle(
            request(Command::SandboxAuthStatus {
                provider: ProviderKind::Devin,
            }),
            EventSink::detached(),
            None,
        )
        .unwrap();
    assert!(matches!(
        status,
        ResponsePayload::SandboxAuthStatus { signed_in: false }
    ));

    let creds = root.join("sandbox-homes/devin/.local/share/devin");
    std::fs::create_dir_all(&creds).unwrap();
    std::fs::write(creds.join("credentials.toml"), "[auth]\n").unwrap();
    let status = backend
        .handle(
            request(Command::SandboxAuthStatus {
                provider: ProviderKind::Devin,
            }),
            EventSink::detached(),
            None,
        )
        .unwrap();
    assert!(matches!(
        status,
        ResponsePayload::SandboxAuthStatus { signed_in: true }
    ));
    std::fs::remove_dir_all(&root).ok();
}

/// Sign-in resolves to the host-side `shuru run` argv the sign-in
/// terminal executes after preparing the real sandbox.
#[test]
#[ignore = "requires Shuru and provider sandbox assets"]
fn sandbox_sign_in_returns_the_guest_invocation() {
    crate::sandbox::shuru_binary().expect("Shuru must be installed for sandbox setup");
    let root = std::env::temp_dir().join(format!("waku-sandbox-signin-{}", Uuid::new_v4()));
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    let response = backend
        .handle(
            waku_protocol::Request {
                request_id: Uuid::new_v4(),
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::SandboxSignIn {
                    provider: ProviderKind::Devin,
                },
            },
            EventSink::detached(),
            None,
        )
        .unwrap();
    let ResponsePayload::SandboxSignIn { program, args, cwd } = response else {
        panic!("expected SandboxSignIn, got {response:?}");
    };
    assert_eq!(program, crate::sandbox::shuru_binary().unwrap());
    assert_eq!(cwd, root.join("sandbox-homes"));
    assert!(args.contains(&"HOME=/root".to_owned()));
    assert!(args.contains(&"--allow-host-writes".to_owned()));
    std::fs::remove_dir_all(&root).ok();
}

/// `agent create` stamps the sender's access posture on the spawned
/// task: sandboxed or supervised work stays contained even when the
/// child resolves another provider. The provider binary override points
/// at a missing path so the first prompt's launch fails deterministically
/// after the session persists, never reaching a real provider process.
#[test]
fn agent_create_inherits_the_senders_access_posture() {
    let root = std::env::temp_dir().join(format!("waku-agent-create-{}", Uuid::new_v4()));
    let project_dir = root.join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
    let mut daemon_settings = settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    settings.replace(daemon_settings).unwrap();
    let backend = WakuBackend::new(settings, StateStore::daemon(root.join("app.db"))).unwrap();
    let create = |sender: Option<Uuid>| {
        backend.create_agent_task(
            sender,
            AgentCreateSelection {
                provider: Some(ProviderKind::Codex),
                model: None,
                title: None,
                reasoning_effort: None,
                service_tier: None,
                context_window: None,
            },
            project_dir.clone(),
            AgentWorkspace::Local,
            None,
            "Summarize the diff".to_owned(),
            &EventSink::detached(),
        )
    };

    let mut sandboxed = AgentSession::new(Uuid::new_v4(), ProviderKind::Claude);
    sandboxed.runtime_mode = crate::model::RuntimeMode::FullAccess;
    sandboxed.environment = crate::model::SessionEnvironment::Sandbox;
    let sandboxed_id = sandboxed.id;
    backend.task_state.lock().sessions.push(sandboxed);
    assert!(create(Some(sandboxed_id)).is_err());
    {
        let state = backend.task_state.lock();
        let created = state
            .sessions
            .last()
            .expect("the spawned task persisted before the failed launch");
        assert_ne!(created.id, sandboxed_id);
        assert_eq!(created.provider, ProviderKind::Codex);
        assert_eq!(created.runtime_mode, crate::model::RuntimeMode::FullAccess);
        assert_eq!(
            created.environment(),
            crate::model::SessionEnvironment::Sandbox
        );
    }

    // A supervised sender cannot relax its spawned work into the more
    // permissive AutoAcceptEdits default.
    let mut supervised = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    supervised.runtime_mode = crate::model::RuntimeMode::Ask;
    let supervised_id = supervised.id;
    backend.task_state.lock().sessions.push(supervised);
    assert!(create(Some(supervised_id)).is_err());
    {
        let state = backend.task_state.lock();
        let created = state.sessions.last().unwrap();
        assert_ne!(created.id, supervised_id);
        assert_eq!(created.runtime_mode, crate::model::RuntimeMode::Ask);
        assert_eq!(
            created.environment(),
            crate::model::SessionEnvironment::Local
        );
    }

    // Senderless creation (the automation scheduler's path) keeps the
    // ordinary defaults.
    assert!(create(None).is_err());
    {
        let state = backend.task_state.lock();
        let created = state.sessions.last().unwrap();
        assert_eq!(
            created.runtime_mode,
            crate::model::RuntimeMode::AutoAcceptEdits
        );
        assert_eq!(
            created.environment(),
            crate::model::SessionEnvironment::Local
        );
    }
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn agent_create_trait_resolution() {
    let options = vec![
        ProviderModelOption::new("medium", "Medium"),
        ProviderModelOption::new("high", "High"),
    ];

    // An explicit id wins, even one the catalog does not list.
    assert_eq!(
        resolve_agent_trait(
            Some("xhigh".into()),
            Some("medium".into()),
            Some(&options),
            Some("medium"),
        ),
        Some("xhigh".into())
    );
    // An explicit "default" (or empty) selects the provider's own
    // default rather than inheriting.
    for value in ["default", ""] {
        assert_eq!(
            resolve_agent_trait(
                Some(value.into()),
                Some("high".into()),
                Some(&options),
                Some("medium"),
            ),
            None
        );
    }
    // An omitted field inherits a value the resolved model still lists.
    assert_eq!(
        resolve_agent_trait(None, Some("high".into()), Some(&options), Some("medium")),
        Some("high".into())
    );
    // An inherited value the model no longer lists falls back to the
    // model's own default, or nothing when it declares none.
    assert_eq!(
        resolve_agent_trait(None, Some("ultra".into()), Some(&options), Some("medium")),
        Some("medium".into())
    );
    assert_eq!(
        resolve_agent_trait(None, Some("ultra".into()), Some(&options), None),
        None
    );
    // A catalog entry without options cannot constrain inheritance.
    assert_eq!(
        resolve_agent_trait(None, Some("ultra".into()), Some(&[]), None),
        Some("ultra".into())
    );
    // No catalog entry at all behaves the same.
    assert_eq!(
        resolve_agent_trait(None, Some("ultra".into()), None, None),
        Some("ultra".into())
    );
}

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
fn a_parked_agent_prompt_mirrors_a_chip_then_cancels_cleanly() {
    let root = std::env::temp_dir().join(format!("waku-queue-{}", Uuid::new_v4()));
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(root.join("repo"));
    // Unstarted drafts own no row; the session must exist on disk for
    // the backend to know it.
    state.sessions[0].begin_turn("seed");
    state.sessions[0].finish_active_turn(TurnStatus::Completed);
    store.save(&mut state).unwrap();
    let session_id = state.sessions[0].id;
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        store,
    )
    .unwrap();

    // A working session parks the prompt — no runtime runs in the test,
    // so the queuedMessagesChanged broadcast is skipped but the mirror
    // still lands in the session document.
    let _ = backend
        .agent
        .note_driver_event(session_id, &DriverEvent::TurnStarted);
    backend
        .queue_agent_prompt(
            session_id,
            "agent follow-up".into(),
            None,
            &EventSink::detached(),
        )
        .unwrap();

    {
        let mut locked = backend.task_state.lock();
        let session = locked
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .unwrap();
        backend.task_store.hydrate(session).unwrap();
        assert_eq!(session.queued_messages.len(), 1);
        assert!(session.queued_messages[0].is_agent_owned());
        assert_eq!(session.queued_messages[0].content, "agent follow-up");
    }

    // A fresh daemon's memory holds nothing; the document rebuilds the
    // parked prompt.
    backend.agent.clear_session(session_id);
    rehydrate_agent_queue(
        &backend.agent,
        &backend.task_state,
        &backend.task_store,
        session_id,
    );
    let restored = backend.agent.pop_queued(session_id).unwrap();
    assert_eq!(restored.prompt, "agent follow-up");
    assert!(restored.queued_id.is_some());

    // Cancel drops the memory entry and the mirrored chip.
    backend.agent.clear_session(session_id);
    let queued_id = restored.queued_id.unwrap();
    rehydrate_agent_queue(
        &backend.agent,
        &backend.task_state,
        &backend.task_store,
        session_id,
    );
    let result = backend
        .cancel_queued_prompt(session_id, queued_id, &EventSink::detached())
        .unwrap();
    assert!(matches!(result, ResponsePayload::Ack));
    assert!(backend.agent.pop_queued(session_id).is_none());
    {
        let mut locked = backend.task_state.lock();
        let session = locked
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .unwrap();
        backend.task_store.hydrate(session).unwrap();
        assert!(session.queued_messages.is_empty());
    }

    // Cancelling a client-owned entry is refused.
    let mut locked = backend.task_state.lock();
    let session = locked
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
        .unwrap();
    session
        .queued_messages
        .push(crate::model::QueuedMessage::new("mine"));
    let user_id = session.queued_messages[0].id;
    drop(locked);
    assert!(
        backend
            .cancel_queued_prompt(session_id, user_id, &EventSink::detached())
            .is_err()
    );

    std::fs::remove_dir_all(root).ok();
}

/// A queued follow-up drains under its chip's id, so the same write
/// that records the delivery retires the parked entry. Without the
/// consumption the document keeps the chip — the next hydrate drains it
/// again and the prompt arrives twice.
#[test]
fn a_delivered_follow_up_leaves_the_persisted_queue() {
    let root = std::env::temp_dir().join(format!("waku-queue-{}", Uuid::new_v4()));
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(root.join("repo"));
    state.sessions[0].begin_turn("seed");
    state.sessions[0].finish_active_turn(TurnStatus::Completed);
    state.sessions[0]
        .queued_messages
        .push(crate::model::QueuedMessage::new("follow up"));
    let queued_id = state.sessions[0].queued_messages[0].id;
    store.save(&mut state).unwrap();
    let session_id = state.sessions[0].id;
    let task_state = Arc::new(Mutex::new(state));
    let task_store = Arc::new(store);

    record_boss_event(
        &task_state,
        &task_store,
        session_id,
        &DriverEvent::PromptSubmitted {
            message: "follow up".into(),
            turn_id: Uuid::new_v4(),
            message_id: queued_id,
            sent_by_task: None,
            hidden: false,
            report_trigger: None,
            reference_context: None,
        },
    )
    .unwrap();

    {
        let locked = task_state.lock();
        let session = &locked.sessions[0];
        assert!(session.queued_messages.is_empty());
        assert_eq!(session.messages.last().unwrap().id, queued_id);
    }

    // The write already landed: a restart hydrates no parked entry.
    let detail = task_store.load_session_detail(session_id).unwrap().unwrap();
    assert!(detail.queued_messages.is_empty());

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn client_saves_cannot_resurrect_or_erase_daemon_queue_entries() {
    let mut existing = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let mut agent_entry = crate::model::QueuedMessage::agent("parked", None);
    agent_entry.created_at = 20;
    let mut user_entry = crate::model::QueuedMessage::new("mine");
    user_entry.created_at = 10;
    existing.queued_messages = vec![user_entry.clone(), agent_entry.clone()];

    // Stale path: a client projection still holding a delivered agent
    // chip must not re-add it — rehydration would deliver it twice.
    let daemon_copy_without_agent = {
        let mut copy = existing.clone();
        copy.queued_messages
            .retain(|queued| !queued.is_agent_owned());
        copy
    };
    let mut resurrecting = daemon_copy_without_agent.clone();
    merge_stale_session_metadata(&mut resurrecting, existing.clone());
    assert!(
        resurrecting
            .queued_messages
            .iter()
            .all(|queued| !queued.is_agent_owned()),
        "a stale client save must not resurrect a daemon-owned entry"
    );

    // Fresh path: a projection written before the mirror arrived keeps
    // the daemon's parked entry instead of erasing it wholesale.
    let mut fresh = daemon_copy_without_agent.clone();
    preserve_daemon_queued_messages(&existing, &mut fresh);
    assert_eq!(
        fresh.queued_messages,
        vec![user_entry, agent_entry],
        "the daemon's agent slice survives a fresh client save"
    );
}

#[test]
fn a_managed_merge_drops_client_removed_queue_entries() {
    let mut existing = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let agent_entry = crate::model::QueuedMessage::agent("parked", None);
    let user_entry = crate::model::QueuedMessage::new("mine");
    existing.queued_messages = vec![user_entry, agent_entry.clone()];

    // Every client save of a managed session takes this path: the
    // projection that dropped its own follow-up is a removal, and the
    // stored copy must drop it too or the chip resurrects when the
    // session next hydrates.
    let mut removal = existing.clone();
    removal
        .queued_messages
        .retain(|queued| queued.is_agent_owned());
    merge_stale_session_metadata(&mut existing, removal);
    assert_eq!(existing.queued_messages, vec![agent_entry.clone()]);

    // A skeleton carries no queue truth — its cleared slice must not
    // strip a user entry the client still holds.
    let mut skeleton = existing.clone();
    skeleton.detail_loaded = false;
    skeleton.queued_messages.clear();
    existing
        .queued_messages
        .push(crate::model::QueuedMessage::new("queued later"));
    merge_stale_session_metadata(&mut existing, skeleton);
    assert_eq!(existing.queued_messages.len(), 2);
    assert_eq!(existing.queued_messages[0], agent_entry);
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
    {
        let mut state = task_state.lock();
        state.last_provider = ProviderKind::Grok;
        state.last_model = Some("grok-code-fast-1".into());
    }
    let transfer = waku_protocol::friends::TransferInfo {
        id: Uuid::new_v4(),
        direction: waku_protocol::friends::TransferDirection::Incoming,
        peer_id: "peer".into(),
        peer_name: "maya".into(),
        // The sender's display title — not the payload's file name.
        title: "new season mockups".into(),
        file_name: Some("design.pdf".into()),
        note: Some("here's the new mockups".into()),
        status: waku_protocol::friends::TransferStatus::Done,
        bytes_done: 12,
        bytes_total: 12,
        dest_dir: Some(share_dir.join("transfers/x")),
        session_id: None,
    };
    let payload_file = share_dir.join("transfers/x/design.pdf");
    std::fs::create_dir_all(payload_file.parent().unwrap()).unwrap();
    std::fs::write(&payload_file, b"hello world!").unwrap();

    let session_id =
        create_transfer_session(&task_state, &store, &share_dir, &transfer, "maya").unwrap();

    {
        let state = task_state.lock();
        let project = state
            .projects
            .iter()
            .find(|project| project.is_friends())
            .expect("the pooled Friends project");
        assert_eq!(project.name, "Friends");
        assert_eq!(project.path, share_dir);
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .expect("the transfer's session");
        assert!(session.quarantined, "received files start untrusted");
        assert!(
            session.environment().is_sandbox(),
            "received files run in the sandbox VM once trusted"
        );
        assert_eq!(session.status, SessionStatus::Idle);
        assert_eq!(session.provider, ProviderKind::Grok);
        assert_eq!(session.model.as_deref(), Some("grok-code-fast-1"));
        assert!(!session.provider_locked());
        assert!(session.can_choose_model(ProviderKind::Claude));
        assert_eq!(session.project_id, project.id);
        assert_eq!(session.title, "new season mockups");
        assert_eq!(session.friend_peer_id.as_deref(), Some("peer"));
        assert_eq!(session.friend_peer_name.as_deref(), Some("maya"));
        let receipt = &session.turns[0];
        assert_eq!(receipt.status, crate::model::TurnStatus::Completed);
        // Note and receipt are assistant messages — bot-style blocks,
        // not a sent bubble — with the note above the delivery details.
        assert!(
            session
                .messages
                .iter()
                .all(|message| message.role == crate::model::MessageRole::Assistant)
        );
        let note_index = session
            .messages
            .iter()
            .position(|message| message.content == "here's the new mockups")
            .expect("the sender's note message");
        let receipt_index = session
            .messages
            .iter()
            .position(|message| message.content.contains("transfers/x"))
            .expect("the delivery receipt message");
        assert!(note_index < receipt_index, "the note lands above the file");
        // The receipt carries the structured payload manifest alongside
        // the plain text the model and older clients still read.
        let Some(crate::model::TranscriptNotice::TransferReceived {
            peer_name,
            title,
            path,
            is_dir,
            is_image,
            size_bytes,
            entries,
            entry_count,
        }) = &session.messages[receipt_index].notice
        else {
            panic!("the receipt carries a TransferReceived notice");
        };
        assert_eq!(peer_name, "maya");
        assert_eq!(title, "design.pdf");
        assert_eq!(*path, payload_file);
        assert!(!is_dir);
        assert!(!is_image);
        assert_eq!(*size_bytes, 12);
        assert!(entries.is_empty());
        assert_eq!(*entry_count, 0);
    }

    // A folder payload's notice lists its children, directories first.
    let folder_dir = share_dir.join("transfers/y/mockups");
    std::fs::create_dir_all(folder_dir.join("sub")).unwrap();
    std::fs::write(folder_dir.join("b.txt"), b"bb").unwrap();
    std::fs::write(folder_dir.join("a.txt"), b"a").unwrap();
    let second = waku_protocol::friends::TransferInfo {
        id: Uuid::new_v4(),
        title: "mockups folder".into(),
        file_name: Some("mockups".into()),
        dest_dir: Some(share_dir.join("transfers/y")),
        ..transfer.clone()
    };
    let second_id =
        create_transfer_session(&task_state, &store, &share_dir, &second, "maya").unwrap();
    {
        let state = task_state.lock();
        // Every sender pools into the one Friends project.
        assert_eq!(
            state
                .projects
                .iter()
                .filter(|project| project.path == share_dir)
                .count(),
            1
        );
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == second_id)
            .expect("the folder transfer's session");
        let receipt = session
            .messages
            .iter()
            .find(|message| message.notice.is_some())
            .expect("the folder receipt");
        let Some(crate::model::TranscriptNotice::TransferReceived {
            path,
            is_dir,
            is_image,
            entries,
            entry_count,
            ..
        }) = &receipt.notice
        else {
            unreachable!();
        };
        assert_eq!(*path, folder_dir);
        assert!(*is_dir);
        assert!(!is_image);
        assert_eq!(*entry_count, 3);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["sub", "a.txt", "b.txt"]);
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].size_bytes, 1);
    }

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
    assert!(!reloaded.sessions[index].environment().is_sandbox());
    reload_store.hydrate(&mut reloaded.sessions[index]).unwrap();
    assert!(reloaded.sessions[index].quarantined);
    assert!(reloaded.sessions[index].environment().is_sandbox());

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn incoming_chat_materializes_a_session() {
    let root = std::env::temp_dir().join(format!("waku-chat-{}", Uuid::new_v4()));
    let share_dir = root.join("share");
    let store = Arc::new(StateStore::daemon(root.join("app.db")));
    let task_state = Arc::new(Mutex::new(PersistedState::fresh(root.join("repo"))));

    let session_id = create_chat_session(
        &task_state,
        &store,
        &share_dir,
        &crate::share::ChatDelivery {
            peer_id: "peer".into(),
            peer_name: "maya".into(),
            title: "shipping the update tonight".into(),
            text: "shipping the update tonight — changelog attached".into(),
        },
    )
    .unwrap();

    let state = task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .expect("the message's session");
    let project_id = state
        .projects
        .iter()
        .find(|project| project.is_friends())
        .expect("the pooled Friends project")
        .id;
    assert_eq!(
        state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.name.as_str()),
        Some("Friends")
    );
    assert_eq!(session.project_id, project_id);
    assert_eq!(session.friend_peer_id.as_deref(), Some("peer"));
    assert_eq!(session.friend_peer_name.as_deref(), Some("maya"));
    // The sender owns the title — it names what was sent, not the peer.
    assert_eq!(session.title, "shipping the update tonight");
    assert_eq!(session.status, SessionStatus::Idle);
    // A message has nothing to quarantine or sandbox — it is just an
    // idle chat holding the friend's text as an agent reply.
    assert!(!session.quarantined);
    assert!(!session.sandboxed);
    assert_eq!(session.messages.len(), 1);
    assert_eq!(
        session.messages[0].role,
        crate::model::MessageRole::Assistant
    );
    assert_eq!(
        session.messages[0].content,
        "shipping the update tonight — changelog attached"
    );
    drop(state);

    // A second message reuses the same friend project; a different
    // sender still lands in the same Friends project.
    create_chat_session(
        &task_state,
        &store,
        &share_dir,
        &crate::share::ChatDelivery {
            peer_id: "peer".into(),
            peer_name: "maya".into(),
            title: "and the pdf".into(),
            text: "and the pdf".into(),
        },
    )
    .unwrap();
    create_chat_session(
        &task_state,
        &store,
        &share_dir,
        &crate::share::ChatDelivery {
            peer_id: "peer-2".into(),
            peer_name: "kai".into(),
            title: "re: deployment".into(),
            text: "did the deploy land?".into(),
        },
    )
    .unwrap();
    {
        let state = task_state.lock();
        assert_eq!(
            state
                .projects
                .iter()
                .filter(|project| project.is_friends())
                .count(),
            1
        );
        assert_eq!(
            state
                .projects
                .iter()
                .filter(|project| project.path == share_dir)
                .count(),
            1
        );
        let kai = state
            .sessions
            .iter()
            .find(|session| session.friend_peer_id.as_deref() == Some("peer-2"))
            .expect("kai's session");
        assert_eq!(kai.friend_peer_name.as_deref(), Some("kai"));
        assert_eq!(kai.project_id, project_id);
    }

    // A display-name change on the next delivery relabels the
    // sender's earlier sessions too.
    create_chat_session(
        &task_state,
        &store,
        &share_dir,
        &crate::share::ChatDelivery {
            peer_id: "peer".into(),
            peer_name: "maya r.".into(),
            title: "one more".into(),
            text: "one more".into(),
        },
    )
    .unwrap();
    let state = task_state.lock();
    assert!(
        state
            .sessions
            .iter()
            .filter(|session| session.friend_peer_id.as_deref() == Some("peer"))
            .all(|session| session.friend_peer_name.as_deref() == Some("maya r."))
    );
    drop(state);

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn removing_a_parent_removes_its_side_chats() {
    let root = std::env::temp_dir().join(format!("waku-side-chats-{}", Uuid::new_v4()));
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(root.join("repo"));
    let parent_id = state.sessions[0].id;
    state.sessions[0].begin_turn("parent");
    state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);

    let project_id = state.projects[0].id;
    let mut side_chat = AgentSession::new(project_id, ProviderKind::Codex);
    side_chat.side_chat_of = Some(parent_id);
    side_chat.begin_turn("side prompt");
    side_chat.finish_active_turn(crate::model::TurnStatus::Completed);
    let side_chat_id = side_chat.id;
    state.push_session(side_chat);
    // A sibling in the same project stays — the cascade follows
    // `side_chat_of`, not shared lineage.
    let mut sibling = AgentSession::new(project_id, ProviderKind::Codex);
    sibling.begin_turn("sibling");
    sibling.finish_active_turn(crate::model::TurnStatus::Completed);
    let sibling_id = sibling.id;
    state.push_session(sibling);
    store.save(&mut state).unwrap();

    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        store,
    )
    .unwrap();
    backend.remove_session(parent_id).unwrap();
    let remaining = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .map(|session| session.id)
        .collect::<Vec<_>>();
    assert_eq!(remaining, vec![sibling_id]);

    // The cascade is durable: the tombstone also keeps a stale client
    // save from resurrecting the child.
    let reloaded = StateStore::daemon(root.join("app.db")).load().unwrap();
    assert!(
        !reloaded
            .sessions
            .iter()
            .any(|session| session.id == side_chat_id)
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn removing_a_project_removes_its_tasks_and_blocks_stale_saves() {
    let root = std::env::temp_dir().join(format!("waku-remove-project-{}", Uuid::new_v4()));
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(root.join("repo"));
    let project = state.projects[0].clone();
    state.sessions[0].begin_turn("remove me");
    state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
    let parent = state.sessions[0].clone();
    let mut side_chat = AgentSession::new(project.id, ProviderKind::Codex);
    side_chat.side_chat_of = Some(parent.id);
    side_chat.begin_turn("side prompt");
    side_chat.finish_active_turn(crate::model::TurnStatus::Completed);
    state.push_session(side_chat);
    let other_project = Project::from_path(root.join("other"));
    let mut other_session = AgentSession::new(other_project.id, ProviderKind::Codex);
    other_session.begin_turn("keep me");
    other_session.finish_active_turn(crate::model::TurnStatus::Completed);
    state.projects.push(other_project.clone());
    state.push_session(other_session.clone());
    store.save(&mut state).unwrap();

    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        store,
    )
    .unwrap();
    backend.remove_project(project.id).unwrap();
    {
        let state = backend.task_state.lock();
        assert_eq!(
            state
                .projects
                .iter()
                .map(|project| project.id)
                .collect::<Vec<_>>(),
            vec![other_project.id]
        );
        assert_eq!(
            state
                .sessions
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            vec![other_session.id]
        );
    }

    let stale = backend
        .handle(
            waku_protocol::Request {
                request_id: Uuid::new_v4(),
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::SaveTaskState {
                    projects: vec![project.clone(), other_project.clone()],
                    live_session_ids: vec![parent.id, other_session.id],
                    sessions: vec![parent.clone(), other_session.clone()],
                    session_tails: Vec::new(),
                },
            },
            EventSink::detached(),
            None,
        )
        .unwrap();
    assert!(matches!(stale, ResponsePayload::TaskStateSaved { .. }));
    {
        let state = backend.task_state.lock();
        assert_eq!(
            state
                .projects
                .iter()
                .map(|project| project.id)
                .collect::<Vec<_>>(),
            vec![other_project.id]
        );
        assert_eq!(
            state
                .sessions
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            vec![other_session.id]
        );
    }

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
        report_trigger: None,
        reference_context: None,
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
        report_trigger: None,
        reference_context: None,
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

#[test]
fn an_agent_can_request_a_project_map_and_get_an_explicit_local_fallback() {
    let root = std::env::temp_dir().join(format!("waku-repo-map-{}", Uuid::new_v4()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/cache.rs"),
        "// Refreshes cached credentials after a token expires.\npub fn refresh_cache() {}\n",
    )
    .unwrap();
    let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
    let backend = WakuBackend::new(settings, StateStore::daemon(root.join("app.db"))).unwrap();
    let session_id = Uuid::new_v4();

    // What spawn_runtime records for an active agent session.
    {
        let mut maps = backend.repo_maps.0.lock();
        maps.sessions.insert(session_id, root.clone());
        maps.indexes.insert(
            root.clone(),
            crate::repo_map::RepoMapIndex::scan(&root).unwrap(),
        );
    }

    let ResponsePayload::AgentProjectMap { result } = backend
        .agent_project_map(
            Some(session_id),
            "refresh credentials",
            None,
            Some(128),
            ProjectMapIntent::Change,
            &["refresh_cache".to_owned()],
            &[],
        )
        .unwrap()
    else {
        panic!("expected an on-demand project map response");
    };
    assert_eq!(result.ranking, ProjectMapRanking::LocalFallback);
    assert!(result.fallback_reason.is_some());
    assert_eq!(result.candidates_considered, 1);
    assert!(result.text.starts_with("src/cache.rs:2\n"));
    assert!(result.text.contains("Refreshes cached credentials"));
    assert!(result.estimated_tokens <= 128);
}

#[test]
fn project_map_ranking_uses_jev_confidence_and_keeps_overflow_paths() {
    let candidates = crate::repo_map::CandidateSet {
        candidates: vec![
            crate::repo_map::MapCandidate {
                path: "src/low.rs".to_owned(),
                evidence: String::new(),
                local_score: 1,
            },
            crate::repo_map::MapCandidate {
                path: "src/high.rs".to_owned(),
                evidence: String::new(),
                local_score: 0,
            },
        ],
        omitted: 1,
        indexed_files: 3,
        omitted_paths: vec!["src/overflow.rs".to_owned()],
    };
    let evaluation = Evaluation {
        model: "test".to_owned(),
        answers: BTreeMap::from([
            ("candidate_000".to_owned(), EvalAnswer::Noul { noul: 0.2 }),
            ("candidate_001".to_owned(), EvalAnswer::Noul { noul: 0.9 }),
        ]),
        usage: Default::default(),
        latency_ms: 0,
        provider_metadata: None,
    };

    let (selected, other) = rank_project_map_candidates(&candidates, &evaluation).unwrap();
    assert_eq!(selected, vec!["src/high.rs"]);
    assert_eq!(other, vec!["src/low.rs", "src/overflow.rs"]);
}

#[test]
fn project_map_candidate_ranking_rejects_missing_or_invalid_judgments() {
    let candidates = crate::repo_map::CandidateSet {
        candidates: vec![crate::repo_map::MapCandidate {
            path: "src/file.rs".to_owned(),
            evidence: String::new(),
            local_score: 0,
        }],
        omitted: 0,
        indexed_files: 1,
        omitted_paths: Vec::new(),
    };
    for noul in [None, Some(f64::NAN), Some(1.1)] {
        let answers = noul
            .map(|noul| BTreeMap::from([("candidate_000".to_owned(), EvalAnswer::Noul { noul })]))
            .unwrap_or_default();
        let evaluation = Evaluation {
            model: "test".to_owned(),
            answers,
            usage: Default::default(),
            latency_ms: 0,
            provider_metadata: None,
        };
        assert!(rank_project_map_candidates(&candidates, &evaluation).is_none());
    }
}

#[test]
fn project_map_paths_must_stay_inside_the_workspace() {
    assert_eq!(
        validate_workspace_relative_path(Path::new("src/auth")).unwrap(),
        PathBuf::from("src/auth")
    );
    assert!(validate_workspace_relative_path(Path::new("../outside")).is_err());
    assert!(validate_workspace_relative_path(Path::new("src/../../outside")).is_err());
    assert!(validate_workspace_relative_path(Path::new("src\\..\\..\\outside")).is_err());
    assert!(validate_workspace_relative_path(Path::new("/tmp/outside")).is_err());
}

/// Records the commands a session's driver receives — the steer path's
/// only observable effect before the provider echoes.
#[derive(Default)]
struct CaptureDriver {
    prompts: Mutex<Vec<String>>,
    steers: Mutex<Vec<String>>,
    shutdowns: Mutex<u32>,
    surface_delivery: crate::driver::AgentSurfaceDelivery,
}

impl crate::driver::DriverControl for CaptureDriver {
    fn prompt(&self, prompt: String) {
        self.prompts.lock().push(prompt);
    }
    fn begin_shutdown(&self) {
        *self.shutdowns.lock() += 1;
    }
    fn supports_steer(&self) -> bool {
        true
    }
    fn agent_surface_delivery(&self) -> crate::driver::AgentSurfaceDelivery {
        self.surface_delivery
    }
    fn steer(&self, prompt: String) {
        self.steers.lock().push(prompt);
    }
    fn respond(&self, _request_id: String, _option_id: String) {}
    fn rollback(
        &self,
        _turns: usize,
    ) -> anyhow::Result<Option<waku_protocol::model::ProviderResumeCursor>> {
        Ok(None)
    }
    fn cancel(&self) {}
}

#[test]
fn the_first_prompt_does_not_auto_load_project_memory() {
    let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
    let memory_store = repo.join(".goddard/memory");
    std::fs::create_dir_all(&memory_store).unwrap();
    std::fs::write(
        memory_store.join("MEMORY.md"),
        "The release freeze lands on Fridays.\n",
    )
    .unwrap();
    std::fs::write(memory_store.join("LOG.txt"), "one durable note\n").unwrap();

    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(repo.clone());
    state.sessions[0].begin_turn("seed");
    state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
    let session_id = state.sessions[0].id;
    store.save(&mut state).unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        store,
    )
    .unwrap();
    // What spawn_runtime records for a fresh session.
    {
        let mut maps = backend.repo_maps.0.lock();
        maps.sessions.insert(session_id, repo.clone());
    }

    let capture = Arc::new(CaptureDriver::default());
    let driver = crate::driver::DriverHandle::from_control(capture.clone());
    backend.steer_first_prompt_context(session_id, "fix the bug", &driver, &EventSink::detached());

    // Project memory is available only through explicit bucket
    // operations; no contents are injected into a task prompt.
    assert!(capture.prompts.lock().is_empty());
    let steers = capture.steers.lock().clone();
    assert!(
        steers
            .iter()
            .all(|steer| !steer.contains("The release freeze lands on Fridays."))
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn boss_prompts_carry_the_header_and_a_pending_verdict_upgrades_to_the_digest() {
    let root = std::env::temp_dir().join(format!("waku-boss-header-{}", Uuid::new_v4()));
    let boss = crate::boss::BossService::open(root.join("boss")).unwrap();
    let boss_id = Uuid::new_v4();
    boss.set_session_id(boss_id).unwrap();

    let mut state = PersistedState::empty();
    let project = Project::from_path(root.join("app"));
    let mut task = AgentSession::new(project.id, ProviderKind::Claude);
    task.set_title("Fix login");
    task.status = SessionStatus::Working;
    // A stored row the daemon never hydrated: started, by definition.
    task.detail_loaded = false;
    state.projects.push(project);
    state.sessions.push(task);
    let task_state = Mutex::new(state);
    let automations = waku_protocol::automations::AutomationsState::default();

    // The first wrap spends the one-shot persona injection.
    let _ = wrap_boss_outbound_prompt(&task_state, &automations, &boss, boss_id, "prime".into());

    // Orientation rides every boss prompt — no router verdict required.
    let wrapped =
        wrap_boss_outbound_prompt(&task_state, &automations, &boss, boss_id, "hello".into());
    assert!(wrapped.contains("<goddard-boss-context>\nWork overview"));
    assert!(wrapped.contains("app: 1 task (1 active)"));
    assert!(!wrapped.contains("Fix login"));
    assert!(wrapped.ends_with("hello"));

    // A deferred attach upgrades the same block to the full digest —
    // once, then the header resumes.
    boss.router_defer_context(boss_id);
    let wrapped =
        wrap_boss_outbound_prompt(&task_state, &automations, &boss, boss_id, "hello".into());
    assert!(wrapped.contains("## app — "));
    assert!(wrapped.contains("\"Fix login\""));
    assert!(!wrapped.contains("Work overview"));
    let wrapped =
        wrap_boss_outbound_prompt(&task_state, &automations, &boss, boss_id, "hello".into());
    assert!(wrapped.contains("Work overview"));

    // Prompts to other sessions are never wrapped.
    let untouched = wrap_boss_outbound_prompt(
        &task_state,
        &automations,
        &boss,
        Uuid::new_v4(),
        "hi".into(),
    );
    assert_eq!(untouched, "hi");

    std::fs::remove_dir_all(root).ok();
}

/// A session plus its side chat in the test store; returns
/// (backend, session_id, side_chat_id). The default test settings keep
/// `agent_tools_enabled` off, so reads exercise the scoped exemption.
fn read_scope_test_backend(root: &Path) -> (WakuBackend, Uuid, Uuid) {
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(repo);
    state.sessions[0].begin_turn("seed");
    state.sessions[0].push_message(crate::model::MessageRole::Assistant, "seeded answer");
    state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
    let session_id = state.sessions[0].id;
    let mut side = crate::model::AgentSession::new(
        state.sessions[0].project_id,
        crate::model::ProviderKind::Codex,
    );
    side.side_chat_of = Some(session_id);
    side.begin_turn("side question");
    side.finish_active_turn(crate::model::TurnStatus::Completed);
    let side_id = side.id;
    state.sessions.push(side);
    store.save(&mut state).unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        store,
    )
    .unwrap();
    (backend, session_id, side_id)
}

#[cfg(unix)]
#[test]
fn computer_use_is_scoped_independently_of_task_and_settings_writes() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = std::env::temp_dir().join(format!("waku-cli-scope-{}", Uuid::new_v4()));
    let (backend, task, other) = read_scope_test_backend(&root);
    let repl = root.join("kernel");
    std::fs::write(
        &repl,
        include_str!("../../../waku-drivers/src/driver/fixtures/computer_use_kernel.py"),
    )
    .unwrap();
    std::fs::set_permissions(&repl, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (events, _received) = crate::driver::test_event_channel();
    let service = crate::driver::bind_computer_use_for_test(
        task,
        repl,
        &root,
        events,
        backend.task_store.blobs(),
    );
    let mut settings = backend.settings.get();
    settings.agent_tools_enabled = false;
    settings.agent_settings_enabled = false;
    settings.computer_use_enabled = true;
    settings.computer_use_experiment_enabled = true;
    backend.settings.replace(settings.clone()).unwrap();
    backend.sessions.lock().insert(
        task,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(Arc::new(CaptureDriver::default())),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: true,
            provider: ProviderKind::Codex,
            cwd: root.join("repo"),
        },
    );
    assert!(matches!(
        backend
            .agent_computer_use(task, Some(task), Some("text"), None, None)
            .unwrap(),
        ResponsePayload::AgentComputerUseResult { .. }
    ));
    assert!(
        backend
            .agent_computer_use(other, Some(task), Some("text"), None, None)
            .unwrap_err()
            .to_string()
            .contains("cannot target another task")
    );
    assert!(
        backend
            .agent_computer_use(other, Some(other), None, None, None)
            .unwrap_err()
            .to_string()
            .contains("unavailable")
    );
    assert!(backend.require_agent_tools().is_err());
    settings.computer_use_experiment_enabled = false;
    backend.settings.replace(settings).unwrap();
    assert!(
        backend
            .agent_computer_use(task, Some(task), None, None, None)
            .unwrap_err()
            .to_string()
            .contains("disabled")
    );
    service.shutdown();
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
#[ignore = "set GODDARD_TEST_AGENT_CLI and GODDARD_TEST_JS_REPL to built executable paths"]
fn computer_use_through_the_built_cli_and_scoped_daemon_connection() {
    let root = std::env::temp_dir().join(format!("waku-cli-wire-{}", Uuid::new_v4()));
    let (backend, task, other) = read_scope_test_backend(&root);
    let (events, _received) = crate::driver::test_event_channel();
    let service = crate::driver::bind_computer_use_for_test(
        task,
        std::env::var_os("GODDARD_TEST_JS_REPL").unwrap().into(),
        &root,
        events,
        backend.task_store.blobs(),
    );
    let mut settings = backend.settings.get();
    settings.agent_tools_enabled = false;
    settings.agent_settings_enabled = false;
    settings.computer_use_enabled = true;
    settings.computer_use_experiment_enabled = true;
    backend.settings.replace(settings).unwrap();
    backend.sessions.lock().insert(
        task,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(Arc::new(CaptureDriver::default())),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: true,
            provider: ProviderKind::Codex,
            cwd: root.join("repo"),
        },
    );
    let token = backend.agent.mint(task);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let backend = Arc::new(backend);
    let server_backend = backend.clone();
    let server = std::thread::spawn(move || {
        crate::server::serve(
            listener,
            "test-desktop-token".into(),
            server_backend,
            server_shutdown,
            crate::server::ServerOptions::default(),
        )
        .unwrap()
    });
    struct Stop(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    let stop = Stop(shutdown);
    let run = |id: Uuid, args: &[&str], stdin: Option<&str>| {
        use std::io::Write as _;
        let mut child =
            std::process::Command::new(std::env::var_os("GODDARD_TEST_AGENT_CLI").unwrap())
                .env("GODDARD_DAEMON_ADDRESS", address.to_string())
                .env("GODDARD_AGENT_TOKEN", &token)
                .env("GODDARD_TASK_ID", id.to_string())
                .args(args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
        if let Some(stdin) = stdin {
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(stdin.as_bytes())
                .unwrap();
        }
        drop(child.stdin.take());
        child.wait_with_output().unwrap()
    };
    let first = run(
        task,
        &[
            "computer",
            "js",
            r#"{"code":"var persistent = 41; jsRepl.write(persistent);"}"#,
        ],
        None,
    );
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&first.stdout).unwrap()["content"][0]["text"],
        "41"
    );
    let next = run(
        task,
        &["computer", "js", "--stdin"],
        Some(r#"{"code":"jsRepl.write(++persistent);"}"#),
    );
    assert!(
        next.status.success(),
        "{}",
        String::from_utf8_lossy(&next.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&next.stdout).unwrap()["content"][0]["text"],
        "42"
    );
    let foreign = run(other, &["computer", "reset"], None);
    assert!(!foreign.status.success());
    assert!(String::from_utf8_lossy(&foreign.stderr).contains("cannot target another task"));
    let error = run(
        task,
        &[
            "computer",
            "js",
            r#"{"code":"throw new Error('expected CLI failure');"}"#,
        ],
        None,
    );
    assert!(!error.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&error.stdout).unwrap()["isError"],
        true
    );
    let image = run(
        task,
        &[
            "computer",
            "js",
            r#"{"code":"await jsRepl.emitImage('data:image/png;base64,aGVsbG8=');"}"#,
        ],
        None,
    );
    assert!(image.status.success());
    let result: Value = serde_json::from_slice(&image.stdout).unwrap();
    let image = result["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "image")
        .unwrap();
    assert_eq!(
        std::fs::read(image["path"].as_str().unwrap()).unwrap(),
        b"hello"
    );
    assert!(run(task, &["computer", "reset"], None).status.success());
    let reset = run(
        task,
        &[
            "computer",
            "js",
            r#"{"code":"jsRepl.write(typeof persistent);"}"#,
        ],
        None,
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&reset.stdout).unwrap()["content"][0]["text"],
        "undefined"
    );
    // Neither unrelated feature becomes reachable merely because CUA works.
    assert!(
        !run(
            task,
            &[
                "prompt",
                r#"{"task_id":"00000000-0000-0000-0000-000000000001","prompt":"no"}"#
            ],
            None
        )
        .status
        .success()
    );
    assert!(
        !run(
            task,
            &["command", "upsert", r#"{"name":"no","script":"true"}"#],
            None
        )
        .status
        .success()
    );
    service.shutdown();
    drop(stop);
    server.join().unwrap();
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn agent_rename_requires_own_task_grant_and_persists_title() {
    let root = std::env::temp_dir().join(format!("waku-agent-rename-{}", Uuid::new_v4()));
    let (backend, session_id, side_id) = read_scope_test_backend(&root);
    let events = EventSink::detached();
    assert!(
        backend
            .agent_rename_self(None, "No caller", &events)
            .is_err()
    );
    // No grant and no runtime to host the request card — refused
    // outright rather than parked where nobody can answer it.
    assert!(
        backend
            .agent_rename_self(Some(session_id), "Denied", &events)
            .is_err()
    );
    {
        let mut state = backend.task_state.lock();
        state.session_mut(session_id).unwrap().agent_rename_allowed = true;
        backend.task_store.save(&mut state).unwrap();
    }
    assert!(
        backend
            .agent_rename_self(Some(session_id), " ", &events)
            .is_err()
    );
    backend
        .agent_rename_self(Some(session_id), "  My title  ", &events)
        .unwrap();
    let state = backend.task_state.lock();
    assert_eq!(
        state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .unwrap()
            .title,
        "My title"
    );
    assert_eq!(
        state
            .sessions
            .iter()
            .find(|session| session.id == side_id)
            .unwrap()
            .title,
        AgentSession::DEFAULT_TITLE
    );
    drop(state);
    let restored = StateStore::daemon(root.join("app.db")).load().unwrap();
    let own = restored
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap();
    assert_eq!(own.title, "My title");
    assert!(own.agent_rename_allowed);
    let _ = std::fs::remove_dir_all(root);
}

/// Without a stored grant the rename parks a permission request on the
/// session; the answer — here "always" — applies the title and records
/// the grant. A finished turn never drains the wait.
#[test]
fn an_ungranted_agent_rename_parks_a_request_until_answered() {
    let root = std::env::temp_dir().join(format!("waku-agent-rename-req-{}", Uuid::new_v4()));
    let (backend, session_id, _side_id) = read_scope_test_backend(&root);
    backend.sessions.lock().insert(
        session_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(Arc::new(CaptureDriver::default())),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.join("repo"),
        },
    );
    let events = EventSink::detached();
    std::thread::scope(|scope| {
        let rename =
            scope.spawn(|| backend.agent_rename_self(Some(session_id), "Fresh title", &events));
        let request_id = loop {
            if let Some(request_id) = backend.agent.parked_permission_request(session_id) {
                break request_id;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(request_id.starts_with(waku_protocol::AGENT_RENAME_REQUEST_PREFIX));
        // A finished turn does not drain the request — the card outlives
        // the fold so it stays answerable.
        backend.agent.note_driver_event(
            session_id,
            &DriverEvent::TurnFinished {
                success: true,
                summary: None,
                summary_i18n: None,
            },
        );
        assert!(
            backend
                .agent
                .parked_permission_request(session_id)
                .is_some()
        );
        assert_eq!(
            backend.agent.resolve_permission(
                session_id,
                &Command::Respond {
                    request_id: request_id.clone(),
                    option_id: "always".into(),
                },
            ),
            Some(request_id)
        );
        assert!(matches!(rename.join().unwrap(), Ok(ResponsePayload::Ack)));
    });
    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap();
    assert_eq!(session.title, "Fresh title");
    assert!(session.agent_rename_allowed);
    let _ = std::fs::remove_dir_all(root);
}

/// A declined request fails the call and leaves the title — and the
/// grant — untouched.
#[test]
fn a_declined_agent_rename_leaves_the_title_alone() {
    let root = std::env::temp_dir().join(format!("waku-agent-rename-deny-{}", Uuid::new_v4()));
    let (backend, session_id, _side_id) = read_scope_test_backend(&root);
    backend.sessions.lock().insert(
        session_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(Arc::new(CaptureDriver::default())),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.join("repo"),
        },
    );
    let events = EventSink::detached();
    std::thread::scope(|scope| {
        let rename = scope.spawn(|| backend.agent_rename_self(Some(session_id), "Nope", &events));
        let request_id = loop {
            if let Some(request_id) = backend.agent.parked_permission_request(session_id) {
                break request_id;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        backend.agent.resolve_permission(
            session_id,
            &Command::Respond {
                request_id,
                option_id: "deny".into(),
            },
        );
        assert!(rename.join().unwrap().is_err());
    });
    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap();
    assert_eq!(session.title, AgentSession::DEFAULT_TITLE);
    assert!(!session.agent_rename_allowed);
    let _ = std::fs::remove_dir_all(root);
}

/// A request the runtime underneath died for resolves unanswered — the
/// parked CLI call errors out instead of waiting on a card nobody can
/// answer.
#[test]
fn an_agent_rename_unparks_when_the_runtime_dies() {
    let root = std::env::temp_dir().join(format!("waku-agent-rename-drain-{}", Uuid::new_v4()));
    let (backend, session_id, _side_id) = read_scope_test_backend(&root);
    backend.sessions.lock().insert(
        session_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(Arc::new(CaptureDriver::default())),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.join("repo"),
        },
    );
    let events = EventSink::detached();
    std::thread::scope(|scope| {
        let rename =
            scope.spawn(|| backend.agent_rename_self(Some(session_id), "Too late", &events));
        loop {
            if backend
                .agent
                .parked_permission_request(session_id)
                .is_some()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        backend
            .agent
            .note_driver_event(session_id, &DriverEvent::ProcessExited);
        assert!(rename.join().unwrap().is_err());
    });
    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap();
    assert_eq!(session.title, AgentSession::DEFAULT_TITLE);
    assert!(!session.agent_rename_allowed);
    let _ = std::fs::remove_dir_all(root);
}

/// Seeds a started sibling task (and its own side chat) plus a task in a
/// second project, beside `read_scope_test_backend`'s caller and side
/// chat. Returns `(sibling, sibling's side chat, foreign, archived)`.
fn archive_proposal_targets(backend: &WakuBackend, root: &Path) -> (Uuid, Uuid, Uuid, Uuid) {
    let mut state = backend.task_state.lock();
    let project_id = state.sessions[0].project_id;
    let started = |project_id| {
        let mut session = AgentSession::new(project_id, ProviderKind::Codex);
        session.begin_turn("seed");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        session
    };
    let target = started(project_id);
    let target_id = target.id;
    let mut target_side = started(project_id);
    target_side.side_chat_of = Some(target_id);
    let target_side_id = target_side.id;
    let foreign_project = Project::from_path(root.join("elsewhere"));
    let foreign = started(foreign_project.id);
    let foreign_id = foreign.id;
    let mut archived = started(project_id);
    archived.archived_at = Some(crate::model::unix_time());
    let archived_id = archived.id;
    state.projects.push(foreign_project);
    state.push_session(target);
    state.push_session(target_side);
    state.push_session(foreign);
    state.push_session(archived);
    drop(state);
    (target_id, target_side_id, foreign_id, archived_id)
}

/// Every refusal happens before a card parks: the gate, an empty or
/// unknown set, a foreign task, a side chat, an already-archived task,
/// and a caller with no runtime to host the request.
#[test]
fn an_archive_proposal_validates_before_parking() {
    let root = std::env::temp_dir().join(format!("waku-agent-archive-val-{}", Uuid::new_v4()));
    let (backend, session_id, side_id) = read_scope_test_backend(&root);
    let (target_id, _target_side_id, foreign_id, archived_id) =
        archive_proposal_targets(&backend, &root);
    let events = EventSink::detached();
    assert!(
        backend
            .agent_propose_archive(None, vec![target_id], None, &events)
            .is_err()
    );
    // The agent surface is off by default — the proposal is refused
    // before anything is even validated.
    assert!(
        backend
            .agent_propose_archive(Some(session_id), vec![target_id], None, &events)
            .is_err()
    );
    let mut settings = backend.settings.get();
    settings.agent_tools_enabled = true;
    backend.settings.replace(settings).unwrap();
    for ids in [
        vec![],
        vec![Uuid::new_v4()],
        vec![foreign_id],
        vec![side_id],
        vec![archived_id],
    ] {
        assert!(
            backend
                .agent_propose_archive(Some(session_id), ids, None, &events)
                .is_err()
        );
    }
    // A valid set still needs a running runtime to show the card on.
    assert!(
        backend
            .agent_propose_archive(
                Some(session_id),
                vec![target_id],
                Some("done".into()),
                &events,
            )
            .is_err()
    );
    assert!(
        backend
            .agent
            .parked_permission_request(session_id)
            .is_none()
    );
    let _ = std::fs::remove_dir_all(root);
}

/// An approved proposal archives the named tasks daemon-side — flag and
/// precedence bump — and deletes their side chats, the same shape a
/// client archive takes.
#[test]
fn an_approved_archive_proposal_archives_tasks_and_side_chats() {
    let root = std::env::temp_dir().join(format!("waku-agent-archive-{}", Uuid::new_v4()));
    let (backend, session_id, _side_id) = read_scope_test_backend(&root);
    let (target_id, target_side_id, _foreign_id, _archived_id) =
        archive_proposal_targets(&backend, &root);
    let mut settings = backend.settings.get();
    settings.agent_tools_enabled = true;
    backend.settings.replace(settings).unwrap();
    backend.sessions.lock().insert(
        session_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(Arc::new(CaptureDriver::default())),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.join("repo"),
        },
    );
    let events = EventSink::detached();
    std::thread::scope(|scope| {
        let proposal = scope.spawn(|| {
            backend.agent_propose_archive(
                Some(session_id),
                vec![target_id],
                Some("finished investigation".into()),
                &events,
            )
        });
        let request_id = loop {
            if let Some(request_id) = backend.agent.parked_permission_request(session_id) {
                break request_id;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(request_id.starts_with(waku_protocol::AGENT_ARCHIVE_REQUEST_PREFIX));
        // A finished turn does not drain the request — the card stays
        // answerable after the fold.
        backend.agent.note_driver_event(
            session_id,
            &DriverEvent::TurnFinished {
                success: true,
                summary: None,
                summary_i18n: None,
            },
        );
        assert!(
            backend
                .agent
                .parked_permission_request(session_id)
                .is_some()
        );
        backend.agent.resolve_permission(
            session_id,
            &Command::Respond {
                request_id,
                option_id: "archive".into(),
            },
        );
        assert!(matches!(proposal.join().unwrap(), Ok(ResponsePayload::Ack)));
    });
    let state = backend.task_state.lock();
    let target = state
        .sessions
        .iter()
        .find(|session| session.id == target_id)
        .unwrap();
    assert!(target.archived_at.is_some());
    assert!(
        state
            .sessions
            .iter()
            .all(|session| session.id != target_side_id)
    );
    assert!(
        state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .unwrap()
            .archived_at
            .is_none()
    );
    drop(state);
    let restored = StateStore::daemon(root.join("app.db")).load().unwrap();
    let stored = restored
        .sessions
        .iter()
        .find(|session| session.id == target_id)
        .unwrap();
    assert!(stored.archived_at.is_some());
    assert!(
        restored
            .sessions
            .iter()
            .all(|session| session.id != target_side_id)
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A declined proposal fails the call and leaves every task untouched.
#[test]
fn a_declined_archive_proposal_archives_nothing() {
    let root = std::env::temp_dir().join(format!("waku-agent-archive-deny-{}", Uuid::new_v4()));
    let (backend, session_id, _side_id) = read_scope_test_backend(&root);
    let (target_id, _target_side_id, _foreign_id, _archived_id) =
        archive_proposal_targets(&backend, &root);
    let mut settings = backend.settings.get();
    settings.agent_tools_enabled = true;
    backend.settings.replace(settings).unwrap();
    backend.sessions.lock().insert(
        session_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(Arc::new(CaptureDriver::default())),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.join("repo"),
        },
    );
    let events = EventSink::detached();
    std::thread::scope(|scope| {
        let proposal = scope.spawn(|| {
            backend.agent_propose_archive(Some(session_id), vec![target_id], None, &events)
        });
        let request_id = loop {
            if let Some(request_id) = backend.agent.parked_permission_request(session_id) {
                break request_id;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        backend.agent.resolve_permission(
            session_id,
            &Command::Respond {
                request_id,
                option_id: "deny".into(),
            },
        );
        assert!(proposal.join().unwrap().is_err());
    });
    let state = backend.task_state.lock();
    assert!(
        state
            .sessions
            .iter()
            .find(|session| session.id == target_id)
            .unwrap()
            .archived_at
            .is_none()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_scoped_read_reaches_self_and_a_side_chats_parent() {
    let root = std::env::temp_dir().join(format!("waku-read-scope-{}", Uuid::new_v4()));
    let (backend, session_id, side_id) = read_scope_test_backend(&root);

    // Self-read works with the task-tools flag off, addressed or bare.
    for (task_id, thread_id) in [(Some(session_id), None), (None, None)] {
        let read = backend
            .agent_read_session(Some(session_id), task_id, thread_id, None, None)
            .expect("a self read is always in scope");
        let ResponsePayload::AgentSessionTranscript { transcript } = read else {
            panic!("expected a transcript");
        };
        assert_eq!(transcript.task_id, session_id);
        assert_eq!(transcript.items.len(), 2);
    }

    // The side chat reads its parent; the reverse direction is not in
    // scope while the flag is off.
    assert!(
        backend
            .agent_read_session(Some(side_id), Some(session_id), None, None, None)
            .is_ok()
    );
    assert!(
        backend
            .agent_read_session(Some(session_id), Some(side_id), None, None, None)
            .is_err()
    );
    // A credential cannot read an unrelated task either.
    assert!(
        backend
            .agent_read_session(Some(session_id), Some(Uuid::new_v4()), None, None, None)
            .is_err()
    );

    // A turn filter returns that turn's entries; a missing turn is a
    // clean error, not an empty transcript.
    let read = backend
        .agent_read_session(Some(session_id), None, None, None, Some(1))
        .expect("turn 1 exists");
    let ResponsePayload::AgentSessionTranscript { transcript } = read else {
        panic!("expected a transcript");
    };
    assert_eq!(transcript.items.len(), 2);
    assert!(transcript.items.iter().all(|item| item.turn == Some(1)));
    let error = backend
        .agent_read_session(Some(session_id), None, None, None, Some(9))
        .unwrap_err();
    assert!(error.to_string().contains("has no turn 9"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resolve_agent_target_reads_cursors_off_the_state_lock() {
    let root = std::env::temp_dir().join(format!("waku-resolve-{}", Uuid::new_v4()));
    let (backend, session_id, _side_id) = read_scope_test_backend(&root);
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(session_id).unwrap();
        // The cursor is detail, so it must land while the session is
        // loaded — a skeleton save would write list columns only.
        backend.task_store.hydrate(session).unwrap();
        session.provider_cursor = Some(crate::model::ProviderResumeCursor::Codex {
            thread_id: "thread-1".to_owned(),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    // Evict the resident detail so the resolver has to read the cursor
    // back from the store — the read used to run under the global lock.
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(session_id).unwrap();
        *session = session.list_projection();
        assert!(!session.detail_loaded);
    }
    let resolved = backend
        .resolve_agent_target(
            None,
            Some("thread-1".to_owned()),
            Some(crate::model::ProviderKind::Codex),
        )
        .unwrap();
    assert_eq!(resolved, session_id);
    {
        let state = backend.task_state.lock();
        let session = state.sessions.iter().find(|s| s.id == session_id).unwrap();
        assert!(session.detail_loaded);
    }
    assert!(
        backend
            .resolve_agent_target(None, Some("no-such-thread".to_owned()), None)
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&root);
}

fn surface_test_backend(root: &Path) -> (WakuBackend, Uuid) {
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(repo);
    state.sessions[0].begin_turn("seed");
    state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
    let session_id = state.sessions[0].id;
    store.save(&mut state).unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        store,
    )
    .unwrap();
    backend.agent.note_surface(
        session_id,
        crate::agent::AgentSurfaceScope {
            task_tools: true,
            settings_writes: true,
            parent_task_id: None,
            boss: false,
        },
    );
    (backend, session_id)
}

#[test]
fn the_agent_surface_instruction_rides_the_context_steer_once() {
    let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let capture = Arc::new(CaptureDriver::default());
    let driver = crate::driver::DriverHandle::from_control(capture.clone());

    backend.steer_first_prompt_context(
        session_id,
        "create a task for this",
        &driver,
        &EventSink::detached(),
    );

    let steers = capture.steers.lock().clone();
    assert_eq!(steers.len(), 1);
    assert!(steers[0].contains("`goddard-agent`"));
    assert!(steers[0].contains("create, start, or spawn"));
    assert!(steers[0].contains("`map` — request Jev-ranked source context"));

    // The accepted echo marks the surface delivered; the next prompt
    // owes no block, so nothing steers at all.
    assert!(
        backend
            .agent
            .take_pending_steer(session_id, &steers[0])
            .is_some()
    );
    backend.agent.mark_surface_announced(session_id);
    backend.steer_first_prompt_context(session_id, "follow up", &driver, &EventSink::detached());
    assert_eq!(capture.steers.lock().len(), 1, "nothing re-injects");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_side_chats_parent_index_rides_the_context_steer_once() {
    let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
    let (backend, session_id, side_id) = read_scope_test_backend(&root);
    // The launch minted the side chat's env — the index names the read
    // surface the credential actually carries.
    backend.agent.note_surface(
        side_id,
        crate::agent::AgentSurfaceScope {
            task_tools: false,
            settings_writes: false,
            parent_task_id: Some(session_id),
            boss: false,
        },
    );
    let capture = Arc::new(CaptureDriver::default());
    let driver = crate::driver::DriverHandle::from_control(capture.clone());

    backend.steer_first_prompt_context(side_id, "side question", &driver, &EventSink::detached());

    let steers = capture.steers.lock().clone();
    assert_eq!(steers.len(), 1);
    let steer = &steers[0];
    assert!(steer.contains("kind=\"index\""));
    // The parent's user text verbatim, its reply as a cue line, and the
    // read invocations pointed at the parent's task id.
    assert!(steer.contains("User: seed"));
    assert!(steer.contains("— Assistant: seeded answer"));
    assert!(steer.contains(&format!("\"task_id\":\"{session_id}\"")));
    assert!(steer.contains("\"turn\":N"));

    // An accepted carry settles the index — the next prompt's steer adds
    // nothing once every other block has also delivered.
    backend.agent.take_pending_steer(side_id, steer).unwrap();
    backend.agent.mark_parent_index_delivered(side_id);
    backend.agent.mark_surface_announced(side_id);
    backend.steer_first_prompt_context(side_id, "follow up", &driver, &EventSink::detached());
    assert_eq!(
        capture.steers.lock().len(),
        1,
        "the index does not re-steer"
    );

    // A session with no parent owes no index at all.
    assert!(backend.side_chat_parent_block(session_id).is_none());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_parent_index_without_the_cli_names_no_read_surface() {
    let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
    let (backend, _session_id, side_id) = read_scope_test_backend(&root);
    // No surface was noted — a daemon that never minted the env — so the
    // index is plain context, not a pointer to a missing command.
    let block = backend.side_chat_parent_block(side_id).unwrap();
    assert!(block.contains("kind=\"index\""));
    assert!(block.contains("User: seed"));
    assert!(!block.contains("goddard-agent read"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_natively_announced_surface_stays_out_of_the_context_steer() {
    let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let capture = Arc::new(CaptureDriver {
        surface_delivery: crate::driver::AgentSurfaceDelivery::Announced,
        ..Default::default()
    });
    let driver = crate::driver::DriverHandle::from_control(capture.clone());

    backend.steer_first_prompt_context(
        session_id,
        "create a task for this",
        &driver,
        &EventSink::detached(),
    );

    // The driver already told the session — no other block is owed, so
    // no steer goes out at all.
    assert!(capture.steers.lock().is_empty());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_agent_surface_prepends_once_for_non_steer_drivers() {
    let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let capture = Arc::new(CaptureDriver::default());
    let driver = crate::driver::DriverHandle::from_control(capture.clone());

    let prompt = backend.prepend_agent_surface(session_id, &driver, "create a task".to_owned());
    assert!(prompt.starts_with("<goddard-agent>"));
    assert!(prompt.ends_with("create a task"));

    let prompt = backend.prepend_agent_surface(session_id, &driver, "next".to_owned());
    assert_eq!(prompt, "next");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_rejected_context_steer_leaves_the_session_eligible() {
    let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let capture = Arc::new(CaptureDriver::default());
    let driver = crate::driver::DriverHandle::from_control(capture.clone());
    backend.steer_first_prompt_context(session_id, "first task", &driver, &EventSink::detached());
    assert_eq!(capture.steers.lock().len(), 1);

    // The steer missed the turn: the pending record drops but the
    // surface flag stays unset, so the next prompt injects again.
    let rejected = backend.agent.note_driver_event(
        session_id,
        &DriverEvent::SteerRejected {
            message: capture.steers.lock()[0].clone(),
            reason: "turn ended".into(),
            reason_i18n: None,
            hidden: false,
        },
    );
    assert!(rejected.is_some_and(|steer| steer.context.is_some()));
    assert!(!backend.agent.context_steer_pending(session_id));

    backend.steer_first_prompt_context(session_id, "next task", &driver, &EventSink::detached());
    assert_eq!(
        capture.steers.lock().len(),
        2,
        "the next prompt retries the context steer"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// A driver that answers nothing; the forwarder only reaches for it
/// while draining a queued prompt, which these tests never do.
struct IdleDriver;

impl driver::DriverControl for IdleDriver {
    fn prompt(&self, _prompt: String) {}
    fn cancel(&self) {}
    fn respond(&self, _request_id: String, _option_id: String) {}
    fn rollback(&self, _turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        Ok(None)
    }
}

#[test]
fn an_exited_runtime_releases_its_replay_journal() {
    let root = std::env::temp_dir().join(format!("waku-exit-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let session_id = Uuid::new_v4();
    let runtime_id = Uuid::new_v4();
    // The sink a runtime-starting request carries: bound to the session
    // and registered with the hub so emitted events journal.
    let events = EventSink::detached().begin_session_runtime(session_id, runtime_id);
    let (wake, _wakes) = smol::channel::bounded(1);
    let (driver_events, event_receiver) = driver::event_channel(wake);
    let driver = DriverHandle::from_control(Arc::new(IdleDriver));
    let sessions = Arc::new(Mutex::new(HashMap::from([(
        session_id,
        RuntimeEntry {
            runtime_id,
            driver: driver.clone(),
            last_active: std::time::Instant::now(),
            resumable: true,
            computer_use_available: false,
            provider: ProviderKind::Claude,
            cwd: PathBuf::new(),
        },
    )])));
    driver_events
        .send(DriverEvent::TextDelta("streamed".into()))
        .unwrap();
    driver_events.send(DriverEvent::ProcessExited).unwrap();

    let task_state = Arc::new(Mutex::new(PersistedState::empty()));
    let task_store = Arc::new(StateStore::daemon(root.join("app.db")));
    forward_driver_events(
        session_id,
        runtime_id,
        event_receiver,
        events.clone(),
        driver,
        Arc::new(crate::agent::AgentState::default()),
        task_state.clone(),
        task_store.clone(),
        sessions.clone(),
        Arc::new(AutomationService::open(root.join("automations.json")).unwrap()),
        Arc::new(crate::boss::BossService::open(root.join("boss")).unwrap()),
        Arc::new(AutoPromptService::open(root.join("auto-prompts.json")).unwrap()),
        Arc::new((Mutex::new(RepoMaps::default()), Condvar::new())),
    );

    assert!(sessions.lock().is_empty());
    assert_eq!(events.journaled_event_count(session_id), 0);
    std::fs::remove_dir_all(&root).ok();
}

/// Per-token deltas and process-output chunks stream live but never
/// journal: under heavy fan-out the journaled flood could saturate a
/// reconnecting client's bounded queue and get it kicked on the next
/// broadcast. Turn boundaries still journal, so replay keeps structure.
#[test]
fn streaming_deltas_do_not_enter_the_replay_journal() {
    let root = std::env::temp_dir().join(format!("waku-deltas-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let session_id = Uuid::new_v4();
    let runtime_id = Uuid::new_v4();
    let events = EventSink::detached().begin_session_runtime(session_id, runtime_id);
    let (wake, _wakes) = smol::channel::bounded(1);
    let (driver_events, event_receiver) = driver::event_channel(wake);
    let driver = DriverHandle::from_control(Arc::new(IdleDriver));
    let sessions = Arc::new(Mutex::new(HashMap::from([(
        session_id,
        RuntimeEntry {
            runtime_id,
            driver: driver.clone(),
            last_active: std::time::Instant::now(),
            resumable: true,
            computer_use_available: false,
            provider: ProviderKind::Claude,
            cwd: PathBuf::new(),
        },
    )])));
    driver_events
        .send(DriverEvent::TextDelta("streamed".into()))
        .unwrap();
    driver_events
        .send(DriverEvent::ReasoningDelta("thought".into()))
        .unwrap();
    driver_events
        .send(DriverEvent::BackgroundWork(
            BackgroundWorkEvent::OutputDelta {
                key: crate::model::BackgroundWorkKey::new(
                    crate::model::BackgroundWorkKind::Process,
                    "proc-1",
                ),
                delta: "chunk".into(),
            },
        ))
        .unwrap();
    driver_events.send(DriverEvent::TurnStarted).unwrap();
    drop(driver_events);

    let task_state = Arc::new(Mutex::new(PersistedState::empty()));
    let task_store = Arc::new(StateStore::daemon(root.join("app.db")));
    forward_driver_events(
        session_id,
        runtime_id,
        event_receiver,
        events.clone(),
        driver,
        Arc::new(crate::agent::AgentState::default()),
        task_state.clone(),
        task_store.clone(),
        sessions.clone(),
        Arc::new(AutomationService::open(root.join("automations.json")).unwrap()),
        Arc::new(crate::boss::BossService::open(root.join("boss")).unwrap()),
        Arc::new(AutoPromptService::open(root.join("auto-prompts.json")).unwrap()),
        Arc::new((Mutex::new(RepoMaps::default()), Condvar::new())),
    );

    assert_eq!(events.journaled_event_count(session_id), 1);
    std::fs::remove_dir_all(&root).ok();
}

/// Removing a runtime must reach the driver's `begin_shutdown`: the
/// event forwarder keeps a `DriverHandle` clone alive until the driver
/// stops emitting, so teardown that waits on `Drop` would never signal
/// a provider still holding its session — Devin's `session_locked` on
/// the next resume.
#[test]
fn closing_a_session_begins_driver_shutdown_while_a_handle_is_held() {
    let root = std::env::temp_dir().join(format!("waku-close-shutdown-{}", Uuid::new_v4()));
    let (backend, session_id, _side_id) = read_scope_test_backend(&root);
    let runtime_id = Uuid::new_v4();
    let capture = Arc::new(CaptureDriver::default());
    // Stands in for the event forwarder's clone, which outlives removal.
    let forwarder_clone = DriverHandle::from_control(capture.clone());
    backend.sessions.lock().insert(
        session_id,
        RuntimeEntry {
            runtime_id,
            driver: DriverHandle::from_control(capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Devin,
            cwd: root.join("repo"),
        },
    );
    backend
        .handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id,
                runtime_id,
                command: Command::CloseSession,
            },
            EventSink::detached(),
            None,
        )
        .unwrap();
    assert!(backend.sessions.lock().is_empty());
    assert_eq!(
        *capture.shutdowns.lock(),
        1,
        "teardown signaled before the forwarder's clone dropped"
    );
    drop(forwarder_clone);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn removing_a_task_sweeps_its_daemon_terminals() {
    let root = std::env::temp_dir().join(format!("waku-sweep-{}", Uuid::new_v4()));
    let project_dir = root.join("repo");
    let worktree_dir = root.join("worktree");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::create_dir_all(&worktree_dir).unwrap();
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(project_dir.clone());
    let project_id = state.projects[0].id;
    let local_id = state.sessions[0].id;
    // Blank sessions never reach the store; both fixtures need a turn.
    state.sessions[0].begin_turn("seed");
    state.sessions[0].finish_active_turn(TurnStatus::Completed);
    let mut worktree_session = state.new_session(project_id, ProviderKind::Codex);
    worktree_session.workspace = SessionWorkspace::Worktree {
        path: worktree_dir.clone(),
        name: "worktree".into(),
        branch: None,
        base_branch: None,
        adopted_by: None,
    };
    worktree_session.begin_turn("seed");
    worktree_session.finish_active_turn(TurnStatus::Completed);
    let worktree_id = worktree_session.id;
    state.push_session(worktree_session);
    store.save(&mut state).unwrap();

    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        store,
    )
    .unwrap()
    .with_terminal_shell(alacritty_terminal::tty::Shell::new(
        "/bin/sh".into(),
        vec!["-c".into(), "while IFS= read -r line; do :; done".into()],
    ));
    let open = |terminal_id, cwd: PathBuf, owner| {
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id: terminal_id,
                    runtime_id: terminal_id,
                    command: Command::OpenTerminal {
                        cwd,
                        cols: 80,
                        rows: 24,
                        owner,
                    },
                },
                EventSink::detached(),
                None,
            )
            .unwrap();
    };
    // One terminal owned by the local task at the shared project root,
    // one anonymous terminal inside the worktree, one outside both.
    let owned = Uuid::new_v4();
    let worktree_bound = Uuid::new_v4();
    let unrelated = Uuid::new_v4();
    open(owned, project_dir.clone(), Some(local_id));
    open(worktree_bound, worktree_dir.clone(), None);
    open(unrelated, root.clone(), None);
    assert_eq!(backend.terminals.lock().len(), 3);

    // Ownership sweeps even at the shared root, but the anonymous
    // worktree terminal survives — its workspace belongs to a live task.
    backend.remove_session(local_id).unwrap();
    assert!(!backend.terminals.lock().contains_key(&owned));
    assert!(backend.terminals.lock().contains_key(&worktree_bound));
    assert!(backend.terminals.lock().contains_key(&unrelated));

    // Removing the worktree task takes every terminal under its path,
    // tagged or not; the outside terminal is untouched.
    backend.remove_session(worktree_id).unwrap();
    assert!(!backend.terminals.lock().contains_key(&worktree_bound));
    assert!(backend.terminals.lock().contains_key(&unrelated));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_idle_reaper_only_takes_runtimes_that_can_come_back() {
    let root = std::env::temp_dir().join(format!("waku-reaper-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(root.join("repo"));
    let project_id = state.projects[0].id;
    let seed = |session: &mut AgentSession| {
        session.begin_turn("seed");
        session.mark_active_turn_provider_started();
        session.provider_cursor = Some(ProviderResumeCursor::Codex {
            thread_id: "thread".into(),
        });
        session.finish_active_turn(TurnStatus::Completed);
    };
    // Resumable and settled — the only runtime the reaper may take.
    let idle_id = state.sessions[0].id;
    seed(&mut state.sessions[0]);
    // Settled but holding no resume cursor — killing its provider would
    // lose the session context a restart cannot rebuild.
    let mut unresumable = state.new_session(project_id, ProviderKind::Codex);
    unresumable.begin_turn("seed");
    unresumable.finish_active_turn(TurnStatus::Completed);
    let unresumable_id = unresumable.id;
    state.push_session(unresumable);
    // Resumable but mid-turn.
    let mut working = state.new_session(project_id, ProviderKind::Codex);
    seed(&mut working);
    working.begin_turn("go");
    working.mark_active_turn_provider_started();
    working.status = SessionStatus::Working;
    let working_id = working.id;
    state.push_session(working);
    // Resumable and settled, with a prompt still parked for delivery.
    let mut queued = state.new_session(project_id, ProviderKind::Codex);
    seed(&mut queued);
    let queued_id = queued.id;
    state.push_session(queued);
    store.save(&mut state).unwrap();

    let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
    let mut doc = settings.get();
    doc.runtime_idle_timeout_secs = Some(60);
    settings.replace(doc).unwrap();
    let backend = WakuBackend::new(settings, store).unwrap();
    backend.agent.enqueue(
        queued_id,
        crate::agent::AgentPrompt {
            prompt: "parked".into(),
            transport: None,
            sender: None,
            queued_id: Some(Uuid::new_v4()),
            context: None,
            hidden: false,
            report_trigger: None,
        },
    );

    // A two-minute-old stamp clears the configured minute. Resumability
    // rides on the runtime entry — the catalog rows could be skeletons.
    let driver = DriverHandle::from_control(Arc::new(IdleDriver));
    let sessions = Arc::new(Mutex::new(HashMap::from_iter(
        [idle_id, unresumable_id, working_id, queued_id]
            .into_iter()
            .map(|id| {
                (
                    id,
                    RuntimeEntry {
                        runtime_id: Uuid::new_v4(),
                        driver: driver.clone(),
                        last_active: std::time::Instant::now()
                            - std::time::Duration::from_secs(120),
                        resumable: id != unresumable_id,
                        computer_use_available: false,
                        provider: ProviderKind::Claude,
                        cwd: PathBuf::new(),
                    },
                )
            }),
    )));

    let evicted = reap_idle_runtimes(
        &sessions,
        &backend.task_state,
        &backend.settings,
        &backend.agent,
        false,
    );
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0].0, idle_id);
    let remaining = sessions.lock();
    assert!(!remaining.contains_key(&idle_id));
    assert!(remaining.contains_key(&unresumable_id));
    assert!(remaining.contains_key(&working_id));
    assert!(remaining.contains_key(&queued_id));
    drop(remaining);

    // A fresh stamp keeps even a resumable runtime.
    sessions.lock().insert(
        idle_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: driver.clone(),
            last_active: std::time::Instant::now(),
            resumable: true,
            computer_use_available: false,
            provider: ProviderKind::Claude,
            cwd: PathBuf::new(),
        },
    );
    assert!(
        reap_idle_runtimes(
            &sessions,
            &backend.task_state,
            &backend.settings,
            &backend.agent,
            false
        )
        .is_empty()
    );

    // Under memory pressure the age gate lifts: the young resumable
    // runtime sheds while busy, queued, and unresumable work — the
    // protections that make eviction safe — stay untouched.
    let evicted = reap_idle_runtimes(
        &sessions,
        &backend.task_state,
        &backend.settings,
        &backend.agent,
        true,
    );
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0].0, idle_id);
    let remaining = sessions.lock();
    assert!(remaining.contains_key(&unresumable_id));
    assert!(remaining.contains_key(&working_id));
    assert!(remaining.contains_key(&queued_id));
    drop(remaining);

    // An explicit `0` disables eviction outright — pressure is a
    // reason to shed sooner, never a reason to override "never".
    let mut disabled = backend.settings.get();
    disabled.runtime_idle_timeout_secs = Some(0);
    backend.settings.replace(disabled).unwrap();
    sessions.lock().insert(
        idle_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: driver.clone(),
            last_active: std::time::Instant::now(),
            resumable: true,
            computer_use_available: false,
            provider: ProviderKind::Claude,
            cwd: PathBuf::new(),
        },
    );
    assert!(
        reap_idle_runtimes(
            &sessions,
            &backend.task_state,
            &backend.settings,
            &backend.agent,
            true
        )
        .is_empty()
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn an_agent_prompt_envelope_names_its_sending_task() {
    let mut state = PersistedState::empty();
    let target = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let target_id = target.id;
    let mut sender = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    sender.set_title("Fix the flaky test");
    let sender_id = sender.id;
    state.sessions.extend([target, sender]);
    let state = Mutex::new(state);

    // A task-to-task prompt names the sender and ends in the verbatim
    // prompt.
    let wrapped = agent_prompt_envelope(&state, target_id, Some(sender_id), "how is it going?")
        .expect("an attributed prompt wraps");
    assert!(wrapped.contains("the agent of another Goddard task"));
    assert!(wrapped.contains("\"Fix the flaky test\""));
    assert!(wrapped.contains(&sender_id.to_string()));
    assert!(wrapped.ends_with("\n\nhow is it going?"));

    // A side chat of the target gets the warmer relation.
    state
        .lock()
        .sessions
        .iter_mut()
        .find(|session| session.id == sender_id)
        .unwrap()
        .side_chat_of = Some(target_id);
    let wrapped = agent_prompt_envelope(&state, target_id, Some(sender_id), "hi")
        .expect("a side chat's prompt wraps");
    assert!(wrapped.contains("your side chat"));

    // Unattributed, self-addressed, and unknown senders.
    assert!(agent_prompt_envelope(&state, target_id, None, "hi").is_none());
    assert!(agent_prompt_envelope(&state, target_id, Some(target_id), "hi").is_none());
    let unknown = Uuid::new_v4();
    let wrapped = agent_prompt_envelope(&state, target_id, Some(unknown), "hi")
        .expect("a known sender id still wraps without its record");
    assert!(wrapped.contains(&unknown.to_string()));
    assert!(wrapped.contains("the agent of another Goddard task"));
}

#[test]
fn agent_search_stays_inside_the_callers_project() {
    let root = std::env::temp_dir().join(format!("waku-agent-search-{}", Uuid::new_v4()));
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(root.join("repo"));
    // The caller's own task — its prompt does not carry the needle.
    let caller_id = state.sessions[0].id;
    let project_id = state.projects[0].id;
    let project_name = state.projects[0].name.clone();
    state.sessions[0].begin_turn("caller prompt");
    state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
    // A sibling in the same project carries it.
    let mut sibling = AgentSession::new(project_id, ProviderKind::Codex);
    sibling.begin_turn("the rare needle phrase");
    sibling.finish_active_turn(crate::model::TurnStatus::Completed);
    let sibling_id = sibling.id;
    state.push_session(sibling);
    // A task in another project matches the text but is out of scope.
    let other_project = Project::from_path(root.join("other"));
    let mut other = AgentSession::new(other_project.id, ProviderKind::Codex);
    other.begin_turn("the rare needle phrase");
    other.finish_active_turn(crate::model::TurnStatus::Completed);
    let other_id = other.id;
    state.projects.push(other_project.clone());
    state.push_session(other);
    store.save(&mut state).unwrap();

    let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
    let mut daemon_settings = settings.get();
    daemon_settings.agent_tools_enabled = true;
    settings.replace(daemon_settings).unwrap();
    let backend = WakuBackend::new(settings, store).unwrap();

    let hits = |query: &str, agent: Uuid| match backend
        .agent_search_sessions(Some(agent), Uuid::nil(), query, None)
        .unwrap()
    {
        ResponsePayload::AgentSessionSearch { hits } => hits,
        other => panic!("unexpected payload {other:?}"),
    };

    // The needle only surfaces the sibling, never the foreign project.
    assert_eq!(
        hits("rare needle", caller_id)
            .iter()
            .map(|hit| hit.task_id)
            .collect::<Vec<_>>(),
        vec![sibling_id]
    );
    // The same query scoped to the other project's task finds its own
    // sibling instead — the confinement follows the caller.
    assert_eq!(
        hits("rare needle", other_id)
            .iter()
            .map(|hit| hit.task_id)
            .collect::<Vec<_>>(),
        vec![other_id]
    );
    // Naming the caller's own project is accepted; naming a foreign
    // one is an error, not a silent empty result.
    assert_eq!(
        hits(&format!("project:{project_name} rare needle"), caller_id)
            .iter()
            .map(|hit| hit.task_id)
            .collect::<Vec<_>>(),
        vec![sibling_id]
    );
    assert!(
        backend
            .agent_search_sessions(
                Some(caller_id),
                Uuid::nil(),
                "project:other rare needle",
                None
            )
            .is_err()
    );
    // `status:` intersects the project allowlist: every task here is
    // idle, so `busy` finds nothing and an idle-only query lists both.
    assert!(hits("status:busy rare needle", caller_id).is_empty());
    let mut listed = hits("status:idle", caller_id)
        .iter()
        .map(|hit| hit.task_id)
        .collect::<Vec<_>>();
    listed.sort();
    let mut expected = vec![caller_id, sibling_id];
    expected.sort();
    assert_eq!(listed, expected);
    // An anonymous request has nothing to scope to.
    assert!(
        backend
            .agent_search_sessions(None, Uuid::nil(), "rare needle", None)
            .is_err()
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn boss_search_spans_every_project() {
    let root = std::env::temp_dir().join(format!("waku-boss-search-{}", Uuid::new_v4()));
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(root.join("repo"));
    // The boss's own task carries no needle.
    let boss_id = state.sessions[0].id;
    let project_id = state.projects[0].id;
    state.sessions[0].begin_turn("boss prompt");
    state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
    // A sibling project task and a foreign project task both match.
    let mut sibling = AgentSession::new(project_id, ProviderKind::Codex);
    sibling.begin_turn("the rare needle phrase");
    sibling.finish_active_turn(crate::model::TurnStatus::Completed);
    let sibling_id = sibling.id;
    state.push_session(sibling);
    let other_project = Project::from_path(root.join("other"));
    let mut other = AgentSession::new(other_project.id, ProviderKind::Codex);
    other.begin_turn("the rare needle phrase");
    other.finish_active_turn(crate::model::TurnStatus::Completed);
    let other_id = other.id;
    state.projects.push(other_project.clone());
    state.push_session(other);
    store.save(&mut state).unwrap();

    let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
    let mut daemon_settings = settings.get();
    daemon_settings.agent_tools_enabled = true;
    settings.replace(daemon_settings).unwrap();
    let backend = WakuBackend::new(settings, store).unwrap();
    backend.boss.set_session_id(boss_id).unwrap();

    let search = |query: &str| match backend
        .agent_search_sessions(Some(boss_id), Uuid::nil(), query, None)
        .unwrap()
    {
        ResponsePayload::AgentSessionSearch { hits } => hits,
        other => panic!("unexpected payload {other:?}"),
    };

    // One query reaches both projects, and each hit names its project.
    let hits = search("rare needle");
    let mut found = hits.iter().map(|hit| hit.task_id).collect::<Vec<_>>();
    found.sort();
    let mut expected = vec![sibling_id, other_id];
    expected.sort();
    assert_eq!(found, expected);
    assert_eq!(
        hits.iter()
            .find(|hit| hit.task_id == other_id)
            .map(|hit| hit.project.as_str()),
        Some("other")
    );
    // `project:` may name any registered project, not just the boss's.
    assert_eq!(
        search("project:other rare needle")
            .iter()
            .map(|hit| hit.task_id)
            .collect::<Vec<_>>(),
        vec![other_id]
    );
    // An unknown project name is an error, not a silent empty result.
    assert!(
        backend
            .agent_search_sessions(
                Some(boss_id),
                Uuid::nil(),
                "project:ghost rare needle",
                None
            )
            .is_err()
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn boss_transcripts_capture_unopened_employee_turns_and_tool_output() {
    let root = std::env::temp_dir().join(format!("boss-transcript-{}", Uuid::new_v4()));
    let store = StateStore::daemon(root.join("app.db"));
    let state = Mutex::new(PersistedState::fresh(root.clone()));
    let session_id = state.lock().sessions[0].id;
    let events = vec![
        DriverEvent::PromptSubmitted {
            message: "Run release checks".into(),
            turn_id: Uuid::new_v4(),
            message_id: Uuid::new_v4(),
            sent_by_task: None,
            hidden: false,
            report_trigger: None,
            reference_context: None,
        },
        DriverEvent::TurnStarted,
        DriverEvent::TextDelta("Checking ".into()),
        DriverEvent::TextDelta("release".into()),
        DriverEvent::RichActivity(ActivityItem::new(
            Some("tool-1".into()),
            crate::model::ActivityKind::Tool,
            "Tests",
            None,
            false,
        )),
        DriverEvent::RichActivity(
            ActivityItem::new(
                Some("tool-1".into()),
                crate::model::ActivityKind::Tool,
                "Tests",
                None,
                true,
            )
            .with_output(Some("All checks passed".into())),
        ),
        DriverEvent::TextDelta("Ready".into()),
        DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None,
        },
    ];
    for event in events {
        record_boss_event(&state, &store, session_id, &event).unwrap();
    }
    let mut restored = store.load().unwrap();
    let session = restored
        .sessions
        .iter_mut()
        .find(|entry| entry.id == session_id)
        .unwrap();
    store.hydrate(session).unwrap();
    assert_eq!(
        session
            .messages
            .iter()
            .map(|entry| entry.content.as_str())
            .collect::<Vec<_>>(),
        vec!["Run release checks", "Checking release", "Ready"]
    );
    assert_eq!(session.transcript_blocks[0].activities.len(), 1);
    assert_eq!(
        session.transcript_blocks[0].activities[0].output.as_deref(),
        Some("All checks passed")
    );
    assert_eq!(session.turns[0].status, TurnStatus::Completed);
    assert!(!session.transcript_index().is_empty());
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn boss_chat_is_continuous_and_uses_a_private_workspace_without_user_projects() {
    use waku_protocol::boss::{BossOperation, BossResult};
    let root = std::env::temp_dir().join(format!("boss-chat-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    backend.task_state.lock().projects.clear();
    let open = || {
        backend
            .handle_boss_operation(
                None,
                BossOperation::Open {
                    provider: ProviderKind::Codex,
                    model: None,
                    mode: Default::default(),
                },
                &EventSink::detached(),
            )
            .unwrap()
    };
    let BossResult::Session { session, project } = open() else {
        panic!("expected boss session")
    };
    let id = session.id;
    assert_eq!(session.project_id, backend.boss.document().identity.id);
    assert_eq!(project.path, backend.boss.owned_workspace().unwrap());
    assert!(project.path.is_dir());
    assert_eq!(project.path.file_name().unwrap(), "workspace");
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(id).unwrap();
        session.begin_turn("Remember our conversation");
        session.push_message(crate::model::MessageRole::Assistant, "Ready to help");
        session.finish_active_turn(TurnStatus::Completed);
        backend.task_store.save(&mut state).unwrap();
    }
    let BossResult::Session {
        session: reopened,
        project: reopened_project,
    } = open()
    else {
        panic!("expected boss session")
    };
    assert_eq!(reopened.id, id);
    // The reply is the list projection — the transcript stays resident on
    // the daemon and the client hydrates it through the ordinary path.
    assert!(!reopened.detail_loaded);
    assert!(reopened.messages.is_empty());
    assert_eq!(reopened.project_id, project.id);
    assert_eq!(reopened_project.id, project.id);
    {
        let state = backend.task_state.lock();
        let resident = state
            .sessions
            .iter()
            .find(|session| session.id == id)
            .unwrap();
        assert!(resident.detail_loaded);
        assert_eq!(resident.messages.len(), 2);
    }
    // A row whose project drifted is repaired and re-saved on open; the
    // wire reply stays a projection either way.
    {
        let mut state = backend.task_state.lock();
        state
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .unwrap()
            .project_id = Uuid::new_v4();
    }
    let BossResult::Session {
        session: repaired, ..
    } = open()
    else {
        panic!("expected boss session")
    };
    assert_eq!(repaired.id, id);
    assert!(!repaired.detail_loaded);
    assert_eq!(repaired.project_id, project.id);
    {
        let state = backend.task_state.lock();
        let resident = state
            .sessions
            .iter()
            .find(|session| session.id == id)
            .unwrap();
        assert_eq!(resident.project_id, project.id);
        assert_eq!(resident.messages.len(), 2);
    }
    let response = backend
        .handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::LoadTaskState,
            },
            EventSink::detached(),
            None,
        )
        .unwrap();
    let ResponsePayload::TaskState {
        projects, sessions, ..
    } = response
    else {
        panic!("expected catalog")
    };
    assert!(projects.is_empty());
    assert!(sessions.iter().any(|session| session.id == id));
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A boss `terminal` op owes its intent to the client that prompted
/// the boss's turn: a visible prompt on a boss principal session
/// records the submitting subscriber, and the op resolves that
/// session's live runtime for the emit instead of dropping the event
/// on the request's nil runtime.
#[test]
fn boss_terminal_op_owes_the_prompting_client() {
    use waku_protocol::boss::{BossOperation, BossResult};
    let root = std::env::temp_dir().join(format!("boss-terminal-{}", Uuid::new_v4()));
    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    let runtime_id = Uuid::new_v4();
    backend.sessions.lock().insert(
        supervisor,
        RuntimeEntry {
            runtime_id,
            driver: DriverHandle::from_control(Arc::new(CaptureDriver::default())),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );

    // Hidden injections never re-attribute the session; the first
    // visible prompt records subscriber 7 as its client.
    let prompt = |hidden| {
        backend.handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: supervisor,
                runtime_id,
                command: Command::Prompt {
                    prompt: "open me a shell".into(),
                    turn_id: None,
                    message_id: None,
                    hidden,
                    attachments: Vec::new(),
                },
            },
            EventSink::detached().with_source_subscriber(7),
            None,
        )
    };
    prompt(true).unwrap();
    assert!(backend.boss_prompt_subscribers.lock().is_empty());
    prompt(false).unwrap();
    assert_eq!(
        backend.boss_prompt_subscribers.lock().get(&supervisor),
        Some(&7)
    );

    let terminal = || BossOperation::Terminal {
        title: "Dev server".into(),
        cwd: "/work/app".into(),
        command: None,
    };
    // A non-principal caller is refused before any routing.
    assert!(
        backend
            .handle_boss_operation(Some(Uuid::new_v4()), terminal(), &EventSink::detached())
            .is_err()
    );
    let result = backend
        .handle_boss_operation(Some(supervisor), terminal(), &EventSink::detached())
        .unwrap();
    assert!(matches!(result, BossResult::TerminalRequested { .. }));

    // With the runtime gone the op reports the miss instead of
    // silently dropping the intent on a stale stream.
    backend.sessions.lock().remove(&supervisor);
    assert!(
        backend
            .handle_boss_operation(Some(supervisor), terminal(), &EventSink::detached())
            .is_err()
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_clean_employee_finish_retires_runtime_revokes_token_and_stays_silent() {
    let root = std::env::temp_dir().join(format!("boss-finish-{}", Uuid::new_v4()));
    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    let persona = backend.boss.document().personas[1].id;
    let employee = backend
        .boss
        .prepare_employee(
            supervisor,
            persona,
            "Release checks".into(),
            None,
            waku_protocol::boss::EmployeeGoal::Goal,
            None,
        )
        .unwrap();
    let employee_id = employee.session_id;
    backend.boss.add_employee(employee).unwrap();
    let mut child = AgentSession::new(
        backend.task_state.lock().sessions[0].project_id,
        ProviderKind::Codex,
    );
    child.id = employee_id;
    child.begin_turn("Run tests");
    child.push_message(crate::model::MessageRole::Assistant, "Tests passed");
    child.finish_active_turn(TurnStatus::Completed);
    {
        let mut state = backend.task_state.lock();
        state.push_session(child);
        backend.task_store.save(&mut state).unwrap();
    }
    let parent_capture = Arc::new(CaptureDriver::default());
    let child_capture = Arc::new(CaptureDriver::default());
    for (id, capture) in [
        (supervisor, parent_capture.clone()),
        (employee_id, child_capture.clone()),
    ] {
        backend.sessions.lock().insert(
            id,
            RuntimeEntry {
                runtime_id: Uuid::new_v4(),
                driver: DriverHandle::from_control(capture),
                last_active: std::time::Instant::now(),
                resumable: false,
                computer_use_available: false,
                provider: ProviderKind::Codex,
                cwd: root.clone(),
            },
        );
    }
    let token = backend.agent.mint(employee_id);
    assert_eq!(backend.agent.resolve(&token), Some(employee_id));
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    assert!(backend.boss.employee(employee_id).unwrap().expired);
    assert!(backend.agent.resolve(&token).is_none());
    assert!(!backend.sessions.lock().contains_key(&employee_id));
    assert_eq!(*child_capture.shutdowns.lock(), 1);
    // A goal's clean finish is silent: the employee's status and index
    // stay in `view` and `context`, but no prompt burns a supervisor
    // turn — the record lists on the client's Goals page instead.
    assert!(parent_capture.prompts.lock().is_empty());
    assert!(parent_capture.steers.lock().is_empty());
    assert!(backend.boss.require_active(employee_id).is_err());
    let _ = std::fs::remove_dir_all(root);
}

/// Boss fixture shared by the report-gating tests: the boss session is
/// live, one prepared goal employee sits on the roster with a finished
/// transcript, and capture drivers stand in for both runtimes. The goal
/// kind keeps a clean finish silent, so these tests only hear back when
/// something besides the kind forces a report.
fn employee_finish_fixture(
    root: &Path,
) -> (
    WakuBackend,
    Uuid,
    Uuid,
    Arc<CaptureDriver>,
    Arc<CaptureDriver>,
) {
    let (backend, supervisor) = surface_test_backend(root);
    backend.boss.set_session_id(supervisor).unwrap();
    let persona = backend.boss.document().personas[1].id;
    let employee = backend
        .boss
        .prepare_employee(
            supervisor,
            persona,
            "Release checks".into(),
            None,
            waku_protocol::boss::EmployeeGoal::Goal,
            None,
        )
        .unwrap();
    let employee_id = employee.session_id;
    backend.boss.add_employee(employee).unwrap();
    let mut child = AgentSession::new(
        backend.task_state.lock().sessions[0].project_id,
        ProviderKind::Codex,
    );
    child.id = employee_id;
    child.begin_turn("Run tests");
    child.push_message(crate::model::MessageRole::Assistant, "Tests passed");
    child.finish_active_turn(TurnStatus::Completed);
    {
        let mut state = backend.task_state.lock();
        state.push_session(child);
        backend.task_store.save(&mut state).unwrap();
    }
    let parent_capture = Arc::new(CaptureDriver::default());
    let child_capture = Arc::new(CaptureDriver::default());
    for (id, capture) in [
        (supervisor, parent_capture.clone()),
        (employee_id, child_capture.clone()),
    ] {
        backend.sessions.lock().insert(
            id,
            RuntimeEntry {
                runtime_id: Uuid::new_v4(),
                driver: DriverHandle::from_control(capture),
                last_active: std::time::Instant::now(),
                resumable: false,
                computer_use_available: false,
                provider: ProviderKind::Codex,
                cwd: root.to_path_buf(),
            },
        );
    }
    (
        backend,
        supervisor,
        employee_id,
        parent_capture,
        child_capture,
    )
}

/// The settle signal classifies the expiry on the durable record —
/// every cause lands with its resumable verdict so a bare dead row
/// is never the whole story.
#[test]
fn the_settle_signal_classifies_the_expiry() {
    use waku_protocol::boss::{EmployeeSettle, ExpiryCause};
    for (settle, cause, resumable) in [
        (EmployeeSettle::TurnFinished, ExpiryCause::Finished, true),
        (
            EmployeeSettle::ProcessExited { mid_turn: true },
            ExpiryCause::ExitedMidTurn,
            true,
        ),
        (
            EmployeeSettle::ProcessExited { mid_turn: false },
            ExpiryCause::ExitedIdle,
            true,
        ),
        (EmployeeSettle::Stopped, ExpiryCause::Stopped, false),
        (EmployeeSettle::LaunchFailed, ExpiryCause::Failed, true),
        (EmployeeSettle::Restarted, ExpiryCause::Restarted, true),
    ] {
        let root = std::env::temp_dir().join(format!("boss-settle-{cause:?}-{}", Uuid::new_v4()));
        let (backend, _supervisor, employee_id, _parent, _child) = employee_finish_fixture(&root);
        if matches!(settle, EmployeeSettle::Stopped) {
            backend.boss.mark_cancelled(employee_id).unwrap();
        }
        backend
            .finish_boss_employee(employee_id, false, settle)
            .unwrap();
        let expiry = backend.boss.employee(employee_id).unwrap().expiry.unwrap();
        assert_eq!(expiry.cause, cause, "{settle:?}");
        assert_eq!(expiry.resumable, resumable, "{settle:?}");
        assert_eq!(expiry.parked_prompts, 0, "{settle:?}");
        assert!(expiry.pending_question.is_none(), "{settle:?}");
        let _ = std::fs::remove_dir_all(root);
    }
}

/// A session that failed classifies its turn-finished settle as
/// `failed` rather than clean.
#[test]
fn a_failed_turn_settles_as_failed() {
    use waku_protocol::boss::{EmployeeSettle, ExpiryCause};
    let root = std::env::temp_dir().join(format!("boss-settle-fail-{}", Uuid::new_v4()));
    let (backend, _supervisor, employee_id, _parent, _child) = employee_finish_fixture(&root);
    {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == employee_id)
            .unwrap();
        session.status = SessionStatus::Failed;
        backend.task_store.save(&mut state).unwrap();
    }
    backend
        .finish_boss_employee(employee_id, false, EmployeeSettle::TurnFinished)
        .unwrap();
    assert_eq!(
        backend
            .boss
            .employee(employee_id)
            .unwrap()
            .expiry
            .unwrap()
            .cause,
        ExpiryCause::Failed
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A prompt parked while the employee wound down upgrades the clean
/// settle to `parkedWork` — the record counts what never delivered.
#[test]
fn a_parked_prompt_settles_as_parked_work() {
    use waku_protocol::boss::{BossOperation, EmployeeControl, EmployeeSettle, ExpiryCause};
    let root = std::env::temp_dir().join(format!("boss-settle-parked-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, _parent, _child) = employee_finish_fixture(&root);
    backend
        .agent
        .note_driver_event(employee_id, &DriverEvent::TurnStarted);
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::Prompt {
                    prompt: "queued while working".into(),
                    delivery: Some(crate::protocol::AgentPromptDelivery::Queue),
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    backend
        .finish_boss_employee(employee_id, false, EmployeeSettle::TurnFinished)
        .unwrap();
    let expiry = backend.boss.employee(employee_id).unwrap().expiry.unwrap();
    assert_eq!(expiry.cause, ExpiryCause::ParkedWork);
    assert_eq!(expiry.parked_prompts, 1);
    assert!(expiry.resumable);
    let _ = std::fs::remove_dir_all(root);
}

/// An `agentAsk` still unanswered at expiry upgrades the clean settle
/// to `unansweredAsk` and keeps the question text — while the parked
/// waiter drains as cancelled like before.
#[test]
fn an_unanswered_ask_settles_with_its_question() {
    use waku_protocol::boss::{EmployeeSettle, ExpiryCause};
    let root = std::env::temp_dir().join(format!("boss-settle-ask-{}", Uuid::new_v4()));
    let (backend, _supervisor, employee_id, _parent, _child) = employee_finish_fixture(&root);
    let (settled, settle_rx) = crossbeam_channel::bounded(1);
    assert!(backend.agent.try_park_ask(
        employee_id,
        "ask-1".into(),
        "Ship the release?".into(),
        settled
    ));
    backend
        .finish_boss_employee(employee_id, false, EmployeeSettle::TurnFinished)
        .unwrap();
    let expiry = backend.boss.employee(employee_id).unwrap().expiry.unwrap();
    assert_eq!(expiry.cause, ExpiryCause::UnansweredAsk);
    assert_eq!(
        expiry.pending_question.as_deref(),
        Some("Ship the release?")
    );
    assert!(expiry.resumable);
    assert!(matches!(
        settle_rx.try_recv(),
        Ok(crate::model::AgentAskOutcome::Cancelled)
    ));
    let _ = std::fs::remove_dir_all(root);
}

/// An interrupted settle reports even when the work kind would keep
/// a clean finish silent: the cause, the resumable verdict, and the
/// resume command land on the supervisor — marked `interrupted`.
#[test]
fn an_interrupted_goal_employee_reports_its_cause() {
    use crate::model::ReportTriggerKind;
    use waku_protocol::boss::EmployeeSettle;
    let root = std::env::temp_dir().join(format!("boss-report-restart-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, parent_capture, _child) = employee_finish_fixture(&root);
    backend
        .finish_boss_employee(employee_id, false, EmployeeSettle::Restarted)
        .unwrap();
    let prompts = parent_capture.prompts.lock();
    assert_eq!(
        prompts.len(),
        1,
        "the interruption reports despite Goal kind"
    );
    assert!(prompts[0].contains("daemon restart"), "{}", prompts[0]);
    assert!(prompts[0].contains("resumable"), "{}", prompts[0]);
    assert!(prompts[0].contains(&employee_id.to_string()));
    drop(prompts);
    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == supervisor)
        .unwrap();
    let report = session
        .messages
        .iter()
        .find(|message| message.report_trigger.is_some())
        .expect("the delivered report carries its trigger");
    assert_eq!(
        report.report_trigger.as_ref().unwrap().kind,
        ReportTriggerKind::Interrupted
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A supervisor's own stop reports nothing — the marked cancel reads
/// as terminal intent, not a surprise the boss needs to hear about.
#[test]
fn a_stopped_employee_sends_no_report() {
    use waku_protocol::boss::EmployeeSettle;
    let root = std::env::temp_dir().join(format!("boss-report-stop-{}", Uuid::new_v4()));
    let (backend, _supervisor, employee_id, parent_capture, _child) =
        employee_finish_fixture(&root);
    backend.boss.mark_cancelled(employee_id).unwrap();
    backend
        .finish_boss_employee(employee_id, true, EmployeeSettle::Stopped)
        .unwrap();
    assert!(parent_capture.prompts.lock().is_empty());
    assert!(parent_capture.steers.lock().is_empty());
    let _ = std::fs::remove_dir_all(root);
}

/// Leftover prompt work on an otherwise-clean settle still reports —
/// the supervisor hears the parked count.
#[test]
fn a_parked_work_expiry_reports_the_count() {
    use waku_protocol::boss::{BossOperation, EmployeeControl, EmployeeSettle};
    let root = std::env::temp_dir().join(format!("boss-report-parked-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, parent_capture, _child) = employee_finish_fixture(&root);
    backend
        .agent
        .note_driver_event(employee_id, &DriverEvent::TurnStarted);
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::Prompt {
                    prompt: "queued while working".into(),
                    delivery: Some(crate::protocol::AgentPromptDelivery::Queue),
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    backend
        .finish_boss_employee(employee_id, false, EmployeeSettle::TurnFinished)
        .unwrap();
    let prompts = parent_capture.prompts.lock();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("1 parked prompt"), "{}", prompts[0]);
    let _ = std::fs::remove_dir_all(root);
}

/// An unanswered ask reports its question text — the supervisor sees
/// what the user was being asked when the employee expired.
#[test]
fn an_unanswered_ask_expiry_reports_the_question() {
    use waku_protocol::boss::EmployeeSettle;
    let root = std::env::temp_dir().join(format!("boss-report-ask-{}", Uuid::new_v4()));
    let (backend, _supervisor, employee_id, parent_capture, _child) =
        employee_finish_fixture(&root);
    let (settled, _settle_rx) = crossbeam_channel::bounded(1);
    backend.agent.try_park_ask(
        employee_id,
        "ask-1".into(),
        "Ship the release?".into(),
        settled,
    );
    backend
        .finish_boss_employee(employee_id, false, EmployeeSettle::TurnFinished)
        .unwrap();
    let prompts = parent_capture.prompts.lock();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("unanswered question"), "{}", prompts[0]);
    assert!(prompts[0].contains("Ship the release?"), "{}", prompts[0]);
    let _ = std::fs::remove_dir_all(root);
}

/// An interruption report takes the steer path — a supervisor mid-turn
/// hears about the expired employee immediately rather than after its
/// queue drains.
#[test]
fn an_interruption_report_steers_into_a_busy_supervisor() {
    use waku_protocol::boss::EmployeeSettle;
    let root = std::env::temp_dir().join(format!("boss-report-steer-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, parent_capture, _child) = employee_finish_fixture(&root);
    backend
        .agent
        .note_driver_event(supervisor, &DriverEvent::TurnStarted);
    backend
        .agent
        .note_driver_event(supervisor, &DriverEvent::TurnParked);
    backend
        .finish_boss_employee(employee_id, false, EmployeeSettle::Restarted)
        .unwrap();
    assert_eq!(parent_capture.steers.lock().len(), 1);
    assert!(parent_capture.prompts.lock().is_empty());
    let _ = std::fs::remove_dir_all(root);
}

/// A `finishing` record recovered after a restart keeps the cause it
/// began with — the tail refills leftovers rather than relabelling
/// the settle.
#[test]
fn a_recovered_finish_keeps_its_recorded_cause() {
    use waku_protocol::boss::{EmployeeSettle, ExpiryCause};
    let root = std::env::temp_dir().join(format!("boss-settle-recover-{}", Uuid::new_v4()));
    let (backend, _supervisor, employee_id, _parent, _child) = employee_finish_fixture(&root);
    backend
        .boss
        .begin_finishing(employee_id, false, false, ExpiryCause::ExitedMidTurn)
        .unwrap();
    // Recovery drives the tail directly with the restart signal.
    backend
        .finish_boss_employee_tail(
            employee_id,
            &backend.boss.employee(employee_id).unwrap(),
            EmployeeSettle::Restarted,
        )
        .unwrap();
    assert_eq!(
        backend
            .boss
            .employee(employee_id)
            .unwrap()
            .expiry
            .unwrap()
            .cause,
        ExpiryCause::ExitedMidTurn
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_flagged_employee_finish_delivers_the_index() {
    let root = std::env::temp_dir().join(format!("boss-flagged-{}", Uuid::new_v4()));
    let (backend, _supervisor, employee_id, parent_capture, _child) =
        employee_finish_fixture(&root);
    backend
        .boss
        .set_employee_blocker(employee_id, "needs a release call".into())
        .unwrap();
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    let prompts = parent_capture.prompts.lock();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&employee_id.to_string()));
    assert!(prompts[0].contains("flagged a blocker: needs a release call"));
    assert!(prompts[0].contains("turn 1"));
    assert!(prompts[0].contains("Tests passed"));
    drop(prompts);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_failed_employee_finish_delivers_the_index() {
    let root = std::env::temp_dir().join(format!("boss-failed-{}", Uuid::new_v4()));
    let (backend, _supervisor, employee_id, parent_capture, _child) =
        employee_finish_fixture(&root);
    {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == employee_id)
            .unwrap();
        session.status = SessionStatus::Failed;
        backend.task_store.save(&mut state).unwrap();
    }
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    let prompts = parent_capture.prompts.lock();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&employee_id.to_string()));
    // A failed finish keeps its status instead of washing to idle.
    assert_eq!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == employee_id)
            .unwrap()
            .status,
        SessionStatus::Failed
    );
    let _ = std::fs::remove_dir_all(root);
}

/// The report's wake marker data lands with the delivered prompt: the
/// session document's hidden message carries the event-time snapshot
/// and the turn it opened — the transcript's durable event→turn record.
#[test]
fn an_errand_finish_marks_the_turn_it_opened() {
    use crate::model::{ReportTriggerBoundary, ReportTriggerKind};
    let root = std::env::temp_dir().join(format!("boss-trigger-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, parent_capture, _child) = employee_finish_fixture(&root);
    backend
        .boss
        .set_employee_goal(employee_id, waku_protocol::boss::EmployeeGoal::Errand)
        .unwrap();
    let employee = backend.boss.employee(employee_id).unwrap();
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    assert_eq!(parent_capture.prompts.lock().len(), 1);

    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == supervisor)
        .unwrap();
    let turn_id = session.active_turn_id().expect("the report opened a turn");
    let report = session
        .messages
        .iter()
        .find(|message| message.report_trigger.is_some())
        .expect("the delivered report carries its trigger");
    assert!(report.hidden);
    assert_eq!(report.turn_id, Some(turn_id));
    assert_eq!(report.sent_by_task, Some(employee_id));
    let trigger = report.report_trigger.as_ref().unwrap();
    assert_eq!(trigger.boundary, ReportTriggerBoundary::Opening);
    assert_eq!(trigger.kind, ReportTriggerKind::Finished);
    assert_eq!(trigger.employee, employee_id);
    assert_eq!(trigger.employee_name, employee.identity.name);
    assert_eq!(trigger.job_title, employee.job_title);
    assert_eq!(trigger.event_id, report.id);
    let _ = std::fs::remove_dir_all(root);
}

/// A blocker reported while the supervisor's turn is parked steers in:
/// the pending steer and the transcript record it writes both keep the
/// trigger, marked at the steer boundary rather than a turn opening.
#[test]
fn a_blocker_steer_marks_its_accepted_boundary() {
    use crate::model::{ReportTriggerBoundary, ReportTriggerKind};
    let root = std::env::temp_dir().join(format!("boss-steer-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, parent_capture, _child) = employee_finish_fixture(&root);
    // The supervisor's turn is open and parked — the steer lands in it
    // rather than opening a fresh turn.
    backend
        .agent
        .note_driver_event(supervisor, &DriverEvent::TurnStarted);
    backend
        .agent
        .note_driver_event(supervisor, &DriverEvent::TurnParked);
    backend
        .handle_boss_operation(
            Some(employee_id),
            waku_protocol::boss::BossOperation::ReportBlocker {
                message: "red build".into(),
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert_eq!(parent_capture.steers.lock().len(), 1);

    let steer = backend
        .agent
        .take_pending_steer(supervisor, &parent_capture.steers.lock()[0])
        .expect("the report steer is pending");
    let trigger = steer.report_trigger.as_ref().unwrap().clone();
    assert_eq!(trigger.boundary, ReportTriggerBoundary::Steer);
    assert_eq!(trigger.kind, ReportTriggerKind::Blocker);
    record_agent_steer(
        &backend.task_state,
        &backend.task_store,
        supervisor,
        &steer.prompt,
        employee_id,
        true,
        Some(trigger),
    );

    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == supervisor)
        .unwrap();
    let report = session
        .messages
        .iter()
        .find(|message| message.report_trigger.is_some())
        .expect("the accepted steer records its trigger");
    assert!(report.hidden);
    assert_eq!(
        report.report_trigger.as_ref().unwrap().boundary,
        ReportTriggerBoundary::Steer
    );
    let _ = std::fs::remove_dir_all(root);
}

/// Employee reports wait for the entire source turn, even if the boss
/// queue drains or the source parks between chunks. Rapid updates keep
/// their individual snapshots and order through the durable queue.
#[test]
fn employee_updates_wait_for_the_complete_source_turn() {
    for busy_supervisor in [false, true] {
        let root = std::env::temp_dir().join(format!("boss-stream-{}", Uuid::new_v4()));
        let (backend, supervisor, employee_id, parent, _child) = employee_finish_fixture(&root);
        let events = EventSink::detached();
        if busy_supervisor {
            backend
                .agent
                .note_driver_event(supervisor, &DriverEvent::TurnStarted);
        }
        backend
            .agent
            .note_driver_event(employee_id, &DriverEvent::TurnStarted);
        record_boss_event(
            &backend.task_state,
            &backend.task_store,
            employee_id,
            &DriverEvent::TurnStarted,
        )
        .unwrap();
        record_boss_event(
            &backend.task_state,
            &backend.task_store,
            employee_id,
            &DriverEvent::TextDelta("long report beginning".into()),
        )
        .unwrap();
        for message in ["first update", "second update"] {
            backend
                .handle_boss_operation(
                    Some(employee_id),
                    waku_protocol::boss::BossOperation::ReportBlocker {
                        message: message.into(),
                    },
                    &events,
                )
                .unwrap();
        }
        backend
            .agent
            .note_driver_event(employee_id, &DriverEvent::TurnParked);
        backend.run_summon_scheduler();
        assert!(parent.prompts.lock().is_empty());
        assert!(parent.steers.lock().is_empty());
        let session = backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == supervisor)
            .unwrap()
            .clone();
        assert_eq!(session.queued_messages.len(), 2);
        assert!(session.queued_messages[0].content.contains("first update"));
        assert!(session.queued_messages[1].content.contains("second update"));
        let finished = DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None,
        };
        record_boss_event(
            &backend.task_state,
            &backend.task_store,
            employee_id,
            &DriverEvent::TextDelta(" and final report text".into()),
        )
        .unwrap();
        record_boss_event(
            &backend.task_state,
            &backend.task_store,
            employee_id,
            &finished,
        )
        .unwrap();
        backend.agent.note_driver_event(employee_id, &finished);
        backend.run_summon_scheduler();
        backend.run_summon_scheduler();
        let prompts = if busy_supervisor {
            parent.steers.lock()
        } else {
            parent.prompts.lock()
        };
        assert_eq!(prompts.len(), 2);
        assert!(prompts[0].contains("first update"));
        assert!(prompts[1].contains("second update"));
        assert!(!backend.agent.has_queued(supervisor));
        let state = backend.task_state.lock();
        let source = state
            .sessions
            .iter()
            .find(|session| session.id == employee_id)
            .unwrap();
        assert!(
            source
                .messages
                .last()
                .unwrap()
                .content
                .ends_with("and final report text")
        );
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }
}

/// A report parked behind a busy supervisor keeps its trigger through
/// the durable mirror — a restart rehydrates the queue with the wake
/// record intact.
#[test]
fn a_parked_report_keeps_its_trigger_through_the_mirror() {
    let root = std::env::temp_dir().join(format!("boss-mirror-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, _parent, _child) = employee_finish_fixture(&root);
    backend
        .boss
        .set_employee_goal(employee_id, waku_protocol::boss::EmployeeGoal::Errand)
        .unwrap();
    backend
        .agent
        .note_driver_event(supervisor, &DriverEvent::TurnStarted);
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    assert!(backend.agent.has_queued(supervisor));

    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == supervisor)
        .unwrap();
    let mirrored = session
        .queued_messages
        .iter()
        .find(|queued| queued.report_trigger.is_some())
        .expect("the parked mirror keeps the trigger");
    let queued_id = mirrored.id;
    let event_id = mirrored.report_trigger.as_ref().unwrap().event_id;
    drop(state);

    // The rehydrated queue entry carries the same record forward.
    rehydrate_agent_queue(
        &backend.agent,
        &backend.task_state,
        &backend.task_store,
        supervisor,
    );
    let entry = backend.agent.pop_queued(supervisor).unwrap();
    let trigger = entry.report_trigger.expect("rehydrated trigger");
    assert_eq!(trigger.event_id, event_id);
    assert_eq!(event_id, queued_id);
    let _ = std::fs::remove_dir_all(root);
}

/// The kind fixed at summon — not any persona grant — is what makes a
/// clean finish report: the same fixture, tagged `errand` instead of
/// `goal`, delivers its transcript index to the supervisor.
#[test]
fn an_errand_employee_finish_delivers_the_index() {
    let root = std::env::temp_dir().join(format!("boss-errand-{}", Uuid::new_v4()));
    let (backend, _supervisor, employee_id, parent_capture, _child) =
        employee_finish_fixture(&root);
    backend
        .boss
        .set_employee_goal(employee_id, waku_protocol::boss::EmployeeGoal::Errand)
        .unwrap();
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    let prompts = parent_capture.prompts.lock();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&employee_id.to_string()));
    assert!(!prompts[0].contains("It flagged a blocker:"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn stopping_a_running_employee_still_reports_to_its_supervisor() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let root = std::env::temp_dir().join(format!("boss-running-stop-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, parent_capture, _child) = employee_finish_fixture(&root);
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::Stop,
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert!(backend.boss.employee(employee_id).unwrap().cancelled);
    let prompts = parent_capture.prompts.lock();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&employee_id.to_string()));
    drop(prompts);
    let _ = std::fs::remove_dir_all(root);
}

/// `reportBlocker` is the employee's mid-job attention channel: it
/// steers into the supervisor's open turn when the runtime can take
/// one, flags the record so the finish still reports, and refuses
/// callers that are not live employees.
#[test]
fn report_blocker_interrupts_the_supervisor_and_marks_the_finish() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("boss-blocker-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, parent_capture, _child) = employee_finish_fixture(&root);
    let stranger = Uuid::new_v4();
    assert!(
        backend
            .handle_boss_operation(
                Some(stranger),
                BossOperation::ReportBlocker {
                    message: "intruder".into(),
                },
                &EventSink::detached(),
            )
            .is_err()
    );
    backend
        .agent
        .note_driver_event(supervisor, &DriverEvent::TurnStarted);
    backend
        .handle_boss_operation(
            Some(employee_id),
            BossOperation::ReportBlocker {
                message: "build is red".into(),
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert_eq!(
        backend
            .boss
            .employee(employee_id)
            .unwrap()
            .blocker
            .as_deref(),
        Some("build is red")
    );
    let steers = parent_capture.steers.lock().clone();
    assert_eq!(steers.len(), 1);
    assert!(steers[0].contains("build is red"));
    // The flag survives to the finish, which reports instead of expiring
    // silently — here it parks behind the open turn as a queued prompt.
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    assert!(parent_capture.prompts.lock().is_empty());
    assert!(backend.agent.has_queued(supervisor));
    let finish = backend.agent.pop_queued(supervisor).unwrap();
    assert!(finish.prompt.contains("It flagged a blocker."));
    assert!(!finish.prompt.contains("build is red"));
    assert!(finish.prompt.contains("Its transcript index follows."));
    // And only an employee may flag: the boss itself cannot.
    assert!(
        backend
            .handle_boss_operation(
                Some(supervisor),
                BossOperation::ReportBlocker {
                    message: "self".into(),
                },
                &EventSink::detached(),
            )
            .is_err()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_reported_blocker_is_not_quoted_again_at_expiry() {
    use waku_protocol::boss::{BossOperation, EmployeeSettle};
    for (queued, final_blocker, repeated) in [
        (false, "build is red", true),
        (false, " BUILD   is red. ", true),
        (true, "build is red", true),
        (false, "release needs approval", false),
    ] {
        let root = std::env::temp_dir().join(format!("boss-blocker-dedupe-{}", Uuid::new_v4()));
        let (backend, supervisor, employee, parent, _child) = employee_finish_fixture(&root);
        if queued {
            // A streaming employee's report parks durably until its turn settles.
            backend
                .agent
                .note_driver_event(employee, &DriverEvent::TurnStarted);
        }
        backend
            .handle_boss_operation(
                Some(employee),
                BossOperation::ReportBlocker {
                    message: "build is red".into(),
                },
                &EventSink::detached(),
            )
            .unwrap();
        if queued {
            assert!(parent.prompts.lock().is_empty());
            assert!(backend.agent.has_queued(supervisor));
        } else {
            assert_eq!(parent.prompts.lock().len(), 1);
        }
        backend
            .boss
            .set_employee_blocker(employee, final_blocker.into())
            .unwrap();
        // Force the finish path to consult the durable delivery record rather
        // than relying on loaded transcript messages (as after a restart).
        {
            let mut state = backend.task_state.lock();
            let session = state
                .sessions
                .iter_mut()
                .find(|session| session.id == supervisor)
                .unwrap();
            session.messages.clear();
            session.queued_messages.clear();
            session.detail_loaded = false;
        }
        backend
            .finish_boss_employee(employee, false, EmployeeSettle::TurnFinished)
            .unwrap();
        let state = backend.task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == supervisor)
            .unwrap();
        let finish = session
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .chain(
                session
                    .queued_messages
                    .iter()
                    .map(|message| message.content.as_str()),
            )
            .find(|content| content.contains("has finished and expired"))
            .expect("expiry still reports its transcript index");
        assert!(finish.contains("Its transcript index follows."));
        if repeated {
            assert!(finish.contains("It flagged a blocker."), "{finish}");
            assert!(!finish.contains("build is red"), "{finish}");
            assert!(!finish.contains("It flagged a blocker:"), "{finish}");
        } else {
            assert!(
                finish.contains("It flagged a blocker: release needs approval"),
                "{finish}"
            );
        }
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }
}

/// An expiring employee must tell attached clients its runtime ended —
/// `end_session_runtime` alone strands their driver handle, and a stale
/// handle pins the sidebar's last-known status: the dead employee keeps
/// showing working forever.
#[test]
fn an_expiring_employee_notifies_attached_clients() {
    let root = std::env::temp_dir().join(format!("boss-expire-notify-{}", Uuid::new_v4()));
    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    let persona = backend.boss.document().personas[1].id;
    let employee = backend
        .boss
        .prepare_employee(
            supervisor,
            persona,
            "Release checks".into(),
            None,
            waku_protocol::boss::EmployeeGoal::Goal,
            None,
        )
        .unwrap();
    let employee_id = employee.session_id;
    backend.boss.add_employee(employee).unwrap();
    {
        let mut state = backend.task_state.lock();
        let mut child = AgentSession::new(state.sessions[0].project_id, ProviderKind::Codex);
        child.id = employee_id;
        state.push_session(child);
        backend.task_store.save(&mut state).unwrap();
    }
    let parent_capture = Arc::new(CaptureDriver::default());
    let child_capture = Arc::new(CaptureDriver::default());
    let employee_runtime = Uuid::new_v4();
    for (id, capture, runtime_id) in [
        (supervisor, parent_capture, Uuid::new_v4()),
        (employee_id, child_capture, employee_runtime),
    ] {
        backend.sessions.lock().insert(
            id,
            RuntimeEntry {
                runtime_id,
                driver: DriverHandle::from_control(capture),
                last_active: std::time::Instant::now(),
                resumable: false,
                computer_use_available: false,
                provider: ProviderKind::Codex,
                cwd: root.clone(),
            },
        );
    }
    // The detached sink stands in for `serve`'s hub: the employee's
    // runtime registers on it and the tap plays an attached client.
    let source = EventSink::detached();
    let tapped = source.tapped_events();
    *backend.event_source.lock() = source.begin_session_runtime(employee_id, employee_runtime);
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    match tapped.try_recv() {
        Ok(crate::ServerMessage::Event(event)) => {
            assert_eq!(event.session_id, employee_id);
            assert_eq!(event.runtime_id, employee_runtime);
            assert_eq!(event.event.kind, "processExited");
        }
        other => panic!("expected the runtime-ended event, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(root);
}

/// An acquire parks its caller in the broker wait; the human-facing boss
/// hands waits to employees instead, so the daemon refuses and redirects.
#[test]
fn the_boss_delegates_resource_acquires_to_employees() {
    let root = std::env::temp_dir().join(format!("boss-resources-{}", Uuid::new_v4()));
    let (backend, boss) = surface_test_backend(&root);
    backend.boss.set_session_id(boss).unwrap();
    let error = backend
        .handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: boss,
                runtime_id: Uuid::nil(),
                command: Command::AgentResources {
                    operation: waku_protocol::resources::ResourceOperation::Acquire {
                        resources: Default::default(),
                        purpose: "wait for a device".into(),
                        holder_pid: 1,
                        wait_seconds: 600,
                        parent: None,
                    },
                },
            },
            EventSink::detached(),
            Some(boss),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("summon an employee"),
        "boss acquire should redirect to delegation: {error:#}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_supervisor_prompt_resurrects_an_expired_employee() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let root = std::env::temp_dir().join(format!("boss-revive-{}", Uuid::new_v4()));
    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    let persona = backend.boss.document().personas[1].id;
    let employee = backend
        .boss
        .prepare_employee(
            supervisor,
            persona,
            "Release checks".into(),
            None,
            waku_protocol::boss::EmployeeGoal::Errand,
            None,
        )
        .unwrap();
    let employee_id = employee.session_id;
    let employee_name = employee.identity.name.clone();
    backend.boss.add_employee(employee).unwrap();
    {
        let mut state = backend.task_state.lock();
        let mut child = AgentSession::new(state.sessions[0].project_id, ProviderKind::Codex);
        child.id = employee_id;
        child.begin_turn("Original assignment");
        child.push_message(crate::model::MessageRole::Assistant, "Original findings");
        child.finish_active_turn(TurnStatus::Completed);
        state.push_session(child);
        backend.task_store.save(&mut state).unwrap();
    }
    let parent_capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        supervisor,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(parent_capture),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    assert!(backend.boss.employee(employee_id).unwrap().expired);
    // The expiry pass dropped the employee's runtime; a resurrected
    // prompt cold-starts one in production — a control driver stands
    // in for it here.
    let capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        employee_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::Prompt {
                    prompt: "One more check".into(),
                    delivery: Some(AgentPromptDelivery::Interrupt),
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert!(!backend.boss.employee(employee_id).unwrap().expired);
    assert_eq!(
        backend.boss.employee(employee_id).unwrap().identity.name,
        employee_name
    );
    assert!(
        backend
            .boss
            .employee(employee_id)
            .unwrap()
            .expired_at
            .is_none()
    );
    assert!(backend.boss.require_active(employee_id).is_ok());
    {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == employee_id)
            .unwrap();
        backend.task_store.hydrate(session).unwrap();
        assert!(
            session
                .messages
                .iter()
                .any(|message| message.content == "Original findings")
        );
        assert!(
            session.active_turn_id().is_some(),
            "the revived prompt starts a new turn"
        );
    }
    let prompts = capture.prompts.lock();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("One more check"));
    drop(prompts);

    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    backend
        .boss
        .set_employee_expired_at(employee_id, 1)
        .unwrap();
    assert_eq!(backend.boss.retire_expired(3_601).unwrap().len(), 1);
    assert!(backend.boss.employee(employee_id).is_none());
    assert!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .any(|session| session.id == employee_id)
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A settle-triggered finish drains instead of cutting a live turn:
/// while the employee's provider turn stays open — a prompt that
/// landed between the settle event and the finish pass — the ticket
/// holds at working and the runtime survives, then the open turn's
/// own settle completes the expiry.
#[test]
fn a_settle_expiry_drains_past_an_open_turn() {
    let root = std::env::temp_dir().join(format!("boss-drain-expiry-{}", Uuid::new_v4()));
    let (backend, supervisor) = summon_test_backend(&root);
    let persona = backend.boss.document().personas[0].id;
    let employee = backend
        .boss
        .prepare_employee(
            supervisor,
            persona,
            "Release checks".into(),
            None,
            waku_protocol::boss::EmployeeGoal::Errand,
            None,
        )
        .unwrap();
    let employee_id = employee.session_id;
    backend.boss.add_employee(employee).unwrap();
    {
        let mut state = backend.task_state.lock();
        let mut child = AgentSession::new(state.sessions[0].project_id, ProviderKind::Codex);
        child.id = employee_id;
        child.begin_turn("Assignment");
        state.push_session(child);
        backend.task_store.save(&mut state).unwrap();
    }
    backend.sessions.lock().insert(
        employee_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(Arc::new(CaptureDriver::default())),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );
    let agent = backend.agent.clone();
    backend
        .boss
        .set_session_busy(Arc::new(move |session_id| agent.has_open_turn(session_id)));

    // The settle pass fires while the next turn is already open —
    // expiry drains rather than killing the in-flight tool call.
    backend
        .agent
        .note_driver_event(employee_id, &DriverEvent::TurnStarted);
    backend
        .finish_boss_employee(
            employee_id,
            true,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    let employee = backend.boss.employee(employee_id).unwrap();
    assert!(!employee.expired);
    assert_eq!(
        backend.boss.employee_lifecycle(employee_id),
        Some(waku_protocol::boss::EmployeeLifecycle::Working)
    );
    assert!(
        backend.sessions.lock().contains_key(&employee_id),
        "the runtime survives the deferred finish"
    );

    // The open turn settles: its own finish pass completes the expiry.
    backend.agent.note_driver_event(
        employee_id,
        &DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None,
        },
    );
    backend
        .finish_boss_employee(
            employee_id,
            true,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    assert!(backend.boss.employee(employee_id).unwrap().expired);
    assert!(backend.sessions.lock().get(&employee_id).is_none());
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// The sidebar drops a task from its ordinary rows on `boss_managed`,
/// not roster membership — retirement removes the roster entry, so the
/// stamp is what keeps a retired employee's task hidden. Employees a
/// pre-flag daemon summoned get stamped from the roster at startup.
#[test]
fn a_retired_employees_task_stays_boss_managed() {
    let root = std::env::temp_dir().join(format!("boss-retired-{}", Uuid::new_v4()));
    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    let persona = backend.boss.document().personas[1].id;
    let employee = backend
        .boss
        .prepare_employee(
            supervisor,
            persona,
            "Release checks".into(),
            None,
            waku_protocol::boss::EmployeeGoal::Errand,
            None,
        )
        .unwrap();
    let employee_id = employee.session_id;
    backend.boss.add_employee(employee).unwrap();
    {
        // A pre-flag employee task: persisted and on the roster, but
        // never stamped.
        let mut state = backend.task_state.lock();
        let mut child = AgentSession::new(state.sessions[0].project_id, ProviderKind::Codex);
        child.id = employee_id;
        // Unstarted sessions own no store row; a real employee task
        // carries its assignment's turn.
        child.begin_turn("seed");
        child.finish_active_turn(crate::model::TurnStatus::Completed);
        state.push_session(child);
        backend.task_store.save(&mut state).unwrap();
    }
    drop(backend);
    // The next daemon over the same data stamps the roster's sessions
    // while it constructs.
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    assert!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .any(|session| session.id == employee_id && session.boss_managed)
    );
    backend
        .boss
        .set_employee_expired_at(employee_id, 1)
        .unwrap();
    assert_eq!(backend.boss.retire_expired(3_601).unwrap().len(), 1);
    assert!(backend.boss.employee(employee_id).is_none());
    assert!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == employee_id)
            .is_some_and(|session| session.boss_managed)
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A worktree summon is the same managed worktree `agent create` makes:
/// the employee task runs in the detached checkout and its assignment
/// names that path, not the supervisor's primary checkout. The launch
/// fails on a missing provider binary — after the task and worktree
/// persist — so the assertions read the persisted session.
#[test]
fn boss_summon_runs_the_employee_in_a_managed_worktree() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("boss-worktree-{}", Uuid::new_v4()));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let git = |args: &[&str]| {
        let output = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(&project)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet", "-b", "main"]);
    git(&["config", "core.autocrlf", "false"]);
    std::fs::write(project.join("README.md"), "main\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Goddard Tests",
        "-c",
        "user.email=waku@example.com",
        "commit",
        "--quiet",
        "-m",
        "initial",
    ]);

    let (backend, boss) = surface_test_backend(&root);
    backend.boss.set_session_id(boss).unwrap();
    let mut daemon_settings = backend.settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    backend.settings.replace(daemon_settings).unwrap();

    let persona = backend.boss.document().personas[1].id;
    let result = backend.handle_boss_operation(
        Some(boss),
        BossOperation::Summon {
            persona_id: persona,
            job_title: "Worktree job".into(),
            prompt: "Summarize the diff".into(),
            project: project.display().to_string(),
            provider: Some(ProviderKind::Codex),
            model: None,
            reasoning_effort: None,
            workspace: Some(AgentWorkspace::Worktree),
            base_branch: Some("main".into()),
            adopt_worktree: None,
            permissions: None,
            work_goal: waku_protocol::boss::EmployeeGoal::Errand,
            icon: None,
            resources: None,
            allow_burst: false,
            group_id: None,
            priority: None,
            goal_id: None,
            plan: None,
            item: None,
            request_id: None,
        },
        &EventSink::detached(),
    );
    assert!(result.is_err());
    let mut state = backend.task_state.lock();
    let session = state
        .sessions
        .iter_mut()
        .find(|session| session.id != boss)
        .expect("the employee task persisted before the failed launch");
    backend.task_store.hydrate(session).unwrap();
    let SessionWorkspace::Worktree {
        path, base_branch, ..
    } = &session.workspace
    else {
        panic!("expected a worktree workspace")
    };
    assert_eq!(base_branch.as_deref(), Some("main"));
    let repository = dunce::canonicalize(&project).unwrap();
    let worktrees = repository.parent().unwrap().join("worktrees");
    assert!(path.starts_with(&worktrees));
    assert!(path.is_dir());
    assert!(crate::worktree::is_linked_worktree(path));
    assert!(
        session.messages[0]
            .content
            .contains(&format!("Assigned project: {}", path.display()))
    );
    assert!(backend.boss.is_employee(session.id));
    assert!(session.boss_managed);
    drop(state);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// The summon marker freezes the card's identity — name, avatar seed,
/// job title, and the resolved icon — so the transcript still resolves
/// it after the roster record retires or the task archives. The missing
/// provider binary fails the launch after the marker persists, so the
/// assertions read the supervisor's stored transcript.
#[test]
fn boss_summon_marker_freezes_the_card_identity() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("boss-summon-card-{}", Uuid::new_v4()));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let (backend, boss) = surface_test_backend(&root);
    backend.boss.set_session_id(boss).unwrap();
    let mut daemon_settings = backend.settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    backend.settings.replace(daemon_settings).unwrap();

    let persona = backend.boss.document().personas[1].id;
    let result = backend.handle_boss_operation(
        Some(boss),
        BossOperation::Summon {
            persona_id: persona,
            job_title: "Release checks".into(),
            prompt: "Summarize the diff".into(),
            project: project.display().to_string(),
            provider: Some(ProviderKind::Codex),
            model: None,
            reasoning_effort: None,
            workspace: None,
            base_branch: None,
            adopt_worktree: None,
            permissions: None,
            work_goal: waku_protocol::boss::EmployeeGoal::Errand,
            icon: Some(waku_protocol::custom_commands::CustomCommandIcon::Beaker),
            resources: None,
            allow_burst: false,
            group_id: None,
            priority: None,
            goal_id: None,
            plan: None,
            item: None,
            request_id: None,
        },
        &EventSink::detached(),
    );
    assert!(result.is_err());
    let employee = backend
        .boss
        .document()
        .employees
        .iter()
        .find(|entry| entry.supervisor_id == boss)
        .expect("the summon persisted a roster record")
        .clone();
    let marker = {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == boss)
            .expect("the supervisor session persisted");
        backend.task_store.hydrate(session).unwrap();
        session
            .transcript_blocks
            .iter()
            .flat_map(|block| block.activities.iter())
            .find(|activity| {
                activity.tool_name.as_deref() == Some(waku_protocol::model::BOSS_SUMMON_TOOL_NAME)
            })
            .cloned()
            .expect("the summon marker landed in the transcript")
    };
    let card = waku_protocol::model::BossSummonCard::parse(
        marker
            .arguments
            .as_deref()
            .expect("the marker carries arguments"),
    )
    .expect("the marker arguments parse as a summon card");
    assert_eq!(card.session_id, employee.session_id);
    assert_eq!(card.name, employee.identity.name);
    assert_eq!(card.avatar_seed, employee.identity.avatar_seed);
    assert_eq!(card.job_title, employee.job_title);
    assert_eq!(
        card.icon,
        Some(waku_protocol::custom_commands::CustomCommandIcon::Beaker)
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// Test scaffolding for the worktree-adoption tests: a Git project,
/// a backend whose provider binary is missing so every launch fails
/// after the session persists, and a summon builder.
struct AdoptFixture {
    root: PathBuf,
    project: PathBuf,
    backend: WakuBackend,
    boss: Uuid,
}

impl AdoptFixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("boss-adopt-{label}-{}", Uuid::new_v4()));
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let git = |args: &[&str]| {
            let output = crate::command_env::search_path_command("git")
                .args(args)
                .current_dir(&project)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "--quiet", "-b", "main"]);
        git(&["config", "core.autocrlf", "false"]);
        std::fs::write(project.join("README.md"), "main\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Goddard Tests",
            "-c",
            "user.email=waku@example.com",
            "commit",
            "--quiet",
            "-m",
            "initial",
        ]);

        let (backend, boss) = surface_test_backend(&root);
        backend.boss.set_session_id(boss).unwrap();
        let mut daemon_settings = backend.settings.get();
        daemon_settings.provider_binary_overrides.insert(
            ProviderKind::Codex,
            root.join("missing-codex").display().to_string(),
        );
        backend.settings.replace(daemon_settings).unwrap();
        Self {
            root,
            project,
            backend,
            boss,
        }
    }

    fn summon(
        &self,
        workspace: Option<AgentWorkspace>,
        adopt_worktree: Option<PathBuf>,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        use waku_protocol::boss::BossOperation;
        self.backend.handle_boss_operation(
            Some(self.boss),
            BossOperation::Summon {
                persona_id: self.backend.boss.document().personas[1].id,
                job_title: "Adopt job".into(),
                prompt: "Continue the work".into(),
                project: self.project.display().to_string(),
                provider: Some(ProviderKind::Codex),
                model: None,
                reasoning_effort: None,
                workspace,
                base_branch: Some("main".into()),
                adopt_worktree,
                permissions: None,
                work_goal: waku_protocol::boss::EmployeeGoal::Errand,
                icon: None,
                resources: None,
                allow_burst: false,
                group_id: None,
                priority: None,
                goal_id: None,
                plan: None,
                item: None,
                request_id: None,
            },
            &EventSink::detached(),
        )
    }

    /// The newest non-boss session's workspace — summons land one
    /// each, so index order tracks summon order.
    fn session_workspace(&self, index: usize) -> (Uuid, SessionWorkspace) {
        let state = self.backend.task_state.lock();
        let session = state
            .sessions
            .iter()
            .filter(|session| session.id != self.boss)
            .nth(index)
            .expect("the employee session persisted");
        (session.id, session.workspace.clone())
    }

    fn workspace_path(&self, index: usize) -> PathBuf {
        let (_, workspace) = self.session_workspace(index);
        match workspace {
            SessionWorkspace::Worktree { path, .. } => path,
            other => panic!("expected a worktree workspace, got {other:?}"),
        }
    }
}

impl Drop for AdoptFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// `workspace: "adopt"` hands a finished employee's daemon worktree to
/// a new summon: the adopter's session binds the same checkout with
/// its dirty state untouched, the previous session stops claiming it,
/// and the assignment names the previous owner.
#[test]
fn boss_summon_adopts_a_finished_employees_worktree() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let fixture = AdoptFixture::new("adopt");

    // The first employee launches into a managed worktree, fails to
    // start its provider, and expires — the worktree stays on disk.
    assert!(
        fixture
            .summon(Some(AgentWorkspace::Worktree), None)
            .is_err()
    );
    let (dead_id, _) = fixture.session_workspace(0);
    let worktree_path = fixture.workspace_path(0);
    std::fs::write(worktree_path.join("wip.txt"), "uncommitted work\n").unwrap();

    // The adopter's own launch fails the same way, but the session it
    // leaves behind owns the adopted checkout.
    assert!(
        fixture
            .summon(Some(AgentWorkspace::Adopt), Some(worktree_path.clone()))
            .is_err()
    );
    let (adopter_id, workspace) = fixture.session_workspace(1);
    let SessionWorkspace::Worktree {
        path,
        base_branch,
        adopted_by,
        ..
    } = &workspace
    else {
        panic!("expected a worktree workspace")
    };
    assert_eq!(*path, worktree_path);
    assert_eq!(base_branch.as_deref(), Some("main"));
    assert!(adopted_by.is_none());
    assert_eq!(
        std::fs::read_to_string(worktree_path.join("wip.txt")).unwrap(),
        "uncommitted work\n"
    );

    // Ownership moved: the dead employee's session records the
    // adopter and no longer claims the path.
    let SessionWorkspace::Worktree {
        adopted_by: dead_adopted_by,
        ..
    } = fixture.session_workspace(0).1
    else {
        panic!("expected a worktree workspace")
    };
    assert_eq!(dead_adopted_by, Some(adopter_id));

    // The assignment tells the adopter whose work it inherited.
    {
        let mut state = fixture.backend.task_state.lock();
        let adopter = state
            .sessions
            .iter_mut()
            .find(|session| session.id == adopter_id)
            .unwrap();
        fixture.backend.task_store.hydrate(adopter).unwrap();
        let dead_name = fixture
            .backend
            .boss
            .employee(dead_id)
            .unwrap()
            .identity
            .name;
        assert!(
            adopter.messages.iter().any(|message| message
                .content
                .contains(&format!("adopted from {dead_name}"))),
            "the envelope should name the previous owner"
        );
    }

    // The dead employee cannot resume into the adopted checkout.
    let resume = fixture.backend.handle_boss_operation(
        Some(fixture.boss),
        BossOperation::Control {
            session_id: dead_id,
            action: EmployeeControl::Prompt {
                prompt: "keep going".into(),
                delivery: Some(AgentPromptDelivery::Interrupt),
            },
        },
        &EventSink::detached(),
    );
    let error = format!("{:#}", resume.unwrap_err());
    assert!(error.contains("worktree was adopted"), "{error}");
}

/// Adoption refuses worktrees it cannot safely take: a live owner's,
/// a checkout that is not a daemon worktree of the project repo, a
/// path nobody owns, and a summon missing its field pair.
#[test]
fn boss_summon_rejects_unadoptable_worktrees() {
    let fixture = AdoptFixture::new("reject");
    let adopt = |path: PathBuf| fixture.summon(Some(AgentWorkspace::Adopt), Some(path));

    // A live owner's worktree rejects, naming the holder.
    let live = fixture
        .backend
        .boss
        .prepare_employee(
            fixture.boss,
            fixture.backend.boss.document().personas[1].id,
            "Live job".into(),
            None,
            waku_protocol::boss::EmployeeGoal::Errand,
            None,
        )
        .unwrap();
    let live_id = live.session_id;
    let live_name = live.identity.name.clone();
    fixture.backend.boss.add_employee(live).unwrap();
    let live_worktree = crate::worktree::create(
        &dunce::canonicalize(&fixture.project).unwrap(),
        None,
        Some("main"),
        false,
        &[],
    )
    .unwrap();
    let mut live_session = AgentSession::new(
        fixture
            .backend
            .register_agent_project(&dunce::canonicalize(&fixture.project).unwrap())
            .unwrap()
            .0,
        ProviderKind::Codex,
    );
    live_session.id = live_id;
    live_session.boss_managed = true;
    live_session.workspace = SessionWorkspace::Worktree {
        path: live_worktree.path.clone(),
        name: live_worktree.name,
        branch: None,
        base_branch: Some("main".into()),
        adopted_by: None,
    };
    {
        let mut state = fixture.backend.task_state.lock();
        state.push_session(live_session);
        fixture.backend.task_store.save(&mut state).unwrap();
    }
    let error = format!("{:#}", adopt(live_worktree.path.clone()).unwrap_err());
    assert!(error.contains("still owned"), "{error}");
    assert!(error.contains(&live_name), "{error}");

    // A plain checkout — the project itself — is not a worktree.
    let error = format!("{:#}", adopt(fixture.project.clone()).unwrap_err());
    assert!(error.contains("not a registered worktree"), "{error}");

    // A worktree of a different repository is still not the project's.
    let foreign = fixture.root.join("foreign");
    std::fs::create_dir_all(&foreign).unwrap();
    let git = |args: &[&str]| {
        let output = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(&foreign)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
    };
    git(&["init", "--quiet", "-b", "main"]);
    std::fs::write(foreign.join("f.txt"), "f\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Goddard Tests",
        "-c",
        "user.email=waku@example.com",
        "commit",
        "--quiet",
        "-m",
        "initial",
    ]);
    let foreign_worktree =
        crate::worktree::create(&foreign, None, Some("main"), false, &[]).unwrap();
    let error = format!("{:#}", adopt(foreign_worktree.path).unwrap_err());
    assert!(error.contains("not a registered worktree"), "{error}");

    // A path Git never registered reports as not daemon-managed.
    let error = format!("{:#}", adopt(fixture.root.join("missing")).unwrap_err());
    assert!(error.contains("does not exist"), "{error}");

    // The field pair is validated before anything else.
    let error = format!(
        "{:#}",
        fixture
            .summon(Some(AgentWorkspace::Adopt), None)
            .unwrap_err()
    );
    assert!(error.contains("require an adoptWorktree"), "{error}");
    let error = format!(
        "{:#}",
        fixture
            .summon(
                Some(AgentWorkspace::Worktree),
                Some(fixture.project.clone())
            )
            .unwrap_err()
    );
    assert!(error.contains("only applies"), "{error}");
}

/// A summon's `reasoningEffort` pins the employee's session effort like
/// `setModel` does: the resolved model's catalog bounds it, and an
/// unsupported id fails the summon before the task persists — it does
/// not silently launch the employee at another effort. The missing
/// provider binary fails the launch after a valid summon persists its
/// session, so the assertions read the persisted record.
#[test]
fn boss_summon_validates_the_requested_reasoning_effort() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("boss-effort-{}", Uuid::new_v4()));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let (backend, boss) = surface_test_backend(&root);
    backend.boss.set_session_id(boss).unwrap();
    let mut daemon_settings = backend.settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    backend.settings.replace(daemon_settings).unwrap();

    let persona = backend.boss.document().personas[0].id;
    let summon = |effort: Option<&str>| {
        backend.handle_boss_operation(
            Some(boss),
            BossOperation::Summon {
                persona_id: persona,
                job_title: "Verify".into(),
                prompt: "Check the build".into(),
                project: project.display().to_string(),
                provider: Some(ProviderKind::Codex),
                model: Some("gpt-5.5".into()),
                reasoning_effort: effort.map(str::to_owned),
                workspace: None,
                base_branch: None,
                adopt_worktree: None,
                permissions: None,
                work_goal: waku_protocol::boss::EmployeeGoal::Errand,
                icon: None,
                resources: None,
                allow_burst: false,
                group_id: None,
                priority: None,
                goal_id: None,
                plan: None,
                item: None,
                request_id: None,
            },
            &EventSink::detached(),
        )
    };

    // The fallback catalog's gpt-5.5 lists low/medium/high/xhigh.
    let error = summon(Some("bogus")).unwrap_err().to_string();
    assert!(
        error.contains("reasoning effort \"bogus\" is not supported by model \"gpt-5.5\""),
        "unexpected error: {error}"
    );
    assert_eq!(
        backend.task_state.lock().sessions.len(),
        1,
        "a rejected summon persists no employee task"
    );

    assert!(summon(Some("high")).is_err());
    let session = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|session| session.id != boss)
        .expect("the employee task persisted before the failed launch")
        .clone();
    assert_eq!(session.reasoning_effort.as_deref(), Some("high"));

    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A backend wired for summon-queue tests: the boss session id set,
/// the broker ledger rooted inside the temp dir (never the host's),
/// and every provider binary pointed at a missing path so a granted
/// dispatch fails deterministically at launch.
fn summon_test_backend(root: &Path) -> (WakuBackend, Uuid) {
    let (backend, boss) = surface_test_backend(root);
    backend.boss.set_session_id(boss).unwrap();
    *backend.broker_root.lock() = Some(root.join("broker"));
    let mut daemon_settings = backend.settings.get();
    for provider in [ProviderKind::Claude, ProviderKind::Codex] {
        daemon_settings.provider_binary_overrides.insert(
            provider,
            root.join(format!("missing-{}", provider.id()))
                .display()
                .to_string(),
        );
    }
    backend.settings.replace(daemon_settings).unwrap();
    // Finish reports deliver to the supervisor through the normal
    // prompt path — a control driver stands in for its runtime.
    backend.sessions.lock().insert(
        boss,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(Arc::new(CaptureDriver::default())),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.to_path_buf(),
        },
    );
    (backend, boss)
}

/// Hold one model claim in the test broker as the daemon — the same
/// key admission counts against, so queued tickets see a full pool
/// without a real runtime. Returns `(task, reservation)` for release.
fn hold_model_slot(backend: &WakuBackend, provider: ProviderKind, model: &str) -> (Uuid, Uuid) {
    let task = Uuid::new_v4();
    let reservation = Uuid::new_v4();
    let attempt = backend
        .resource_broker()
        .unwrap()
        .try_admission(
            task,
            reservation,
            waku_protocol::resources::ResourceSet::default(),
            "capacity fixture".into(),
            waku_protocol::resources::AdmissionClaim {
                daemon: backend.boss.document().identity.id,
                provider: provider.id().into(),
                model: model.into(),
                // The fixture's own claim always grants — the limits
                // it pretends to hold gate only itself, while real
                // tickets count the reservation it leaves behind.
                live_limit: u32::MAX,
                hard_cap: u32::MAX,
                allow_burst: false,
            },
        )
        .unwrap();
    assert!(attempt.granted, "fixture slot could not be held");
    (task, reservation)
}

/// Upsert one provider/model rule into the durable policy — the
/// revision comes from the document so repeated calls stay linear.
fn set_model_policy(
    backend: &WakuBackend,
    provider: ProviderKind,
    model: &str,
    live_limit: u32,
    hard_cap: u32,
) {
    let mut policy = backend.boss.document().resource_policy.clone();
    policy
        .model_limits
        .retain(|rule| !(rule.provider == provider && rule.model == model));
    policy.model_limits.push(waku_protocol::boss::ModelLimit {
        provider,
        model: model.into(),
        live_limit,
        hard_cap,
    });
    backend
        .boss
        .set_resource_policy(None, policy.revision, policy.model_limits, policy.host)
        .unwrap();
}

fn summon_op(
    backend: &WakuBackend,
    root: &Path,
    job: &str,
    provider: ProviderKind,
    model: Option<&str>,
) -> waku_protocol::boss::BossOperation {
    waku_protocol::boss::BossOperation::Summon {
        persona_id: backend.boss.document().personas[1].id,
        job_title: job.into(),
        prompt: format!("Work on {job}"),
        project: root.join("repo").display().to_string(),
        provider: Some(provider),
        model: model.map(str::to_owned),
        reasoning_effort: None,
        workspace: None,
        base_branch: None,
        adopt_worktree: None,
        permissions: None,
        work_goal: waku_protocol::boss::EmployeeGoal::Errand,
        icon: None,
        resources: None,
        allow_burst: false,
        group_id: None,
        priority: None,
        goal_id: None,
        plan: None,
        item: None,
        request_id: None,
    }
}

/// At capacity a valid summon is not an error — it is a durable
/// queued ticket that creates no worktree, no runtime, and no claims.
/// The returned admission carries the position and the wait reason.
#[test]
fn a_summon_at_capacity_admits_a_durable_queued_ticket() {
    use waku_protocol::boss::{AdmissionBlocker, BossResult, EmployeeLifecycle};
    let root = std::env::temp_dir().join(format!("summon-queued-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    let (held_task, held) = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    let result = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "queued job",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .expect("at capacity the summon must still be accepted");
    let BossResult::Summoned {
        session_id,
        state,
        admission,
    } = result
    else {
        panic!("expected a summoned result")
    };
    assert_eq!(state, EmployeeLifecycle::Queued);
    let admission = admission.expect("queued results carry an admission");
    assert_eq!(admission.queue_position, Some(1));
    assert!(
        admission
            .blocked_by
            .iter()
            .any(|blocker| matches!(blocker, AdmissionBlocker::ModelLimit { used: 1, limit: 1 })),
        "expected the full pool as the wait reason: {:?}",
        admission.blocked_by
    );

    // Nothing but the shell exists: no runtime, no started turn, and
    // the ledger still holds only the fixture slot.
    assert!(backend.sessions.lock().get(&session_id).is_none());
    {
        let state = backend.task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .expect("the task shell persists for clients");
        assert!(!session.has_started());
    }
    assert!(
        !root
            .join("repo")
            .parent()
            .unwrap()
            .join("worktrees")
            .exists(),
        "a queued ticket must not create worktrees"
    );
    let status = backend
        .resource_broker()
        .unwrap()
        .operate(
            Uuid::new_v4(),
            waku_protocol::resources::ResourceOperation::Status { id: None },
        )
        .unwrap();
    assert_eq!(
        status
            .reservations
            .iter()
            .filter(|reservation| reservation.granted_at.is_some() && !reservation.released)
            .count(),
        1
    );

    // Freeing the pool dispatches the head — here the launch fails on
    // the missing binary and the employee expires with the blocker the
    // finish report carries.
    let supervisor_capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        boss,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(supervisor_capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );
    backend
        .resource_broker()
        .unwrap()
        .release_admission(held_task, held);
    backend.run_summon_scheduler();
    let employee = backend.boss.employee(session_id).unwrap();
    assert!(employee.expired);
    assert!(
        employee
            .blocker
            .as_deref()
            .is_some_and(|note| note.contains("Employee launch failed"))
    );
    let reports = supervisor_capture.prompts.lock();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].contains("Employee launch failed"));
    assert!(!reports[0].contains("has started working"));
    drop(reports);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A queued ticket's unstarted task shell is what the roster click
/// selects — the catalog must project it even though no turn ran.
/// Unstarted rows the boss does not manage stay excluded: they are
/// client drafts, not selectable surfaces.
#[test]
fn the_task_catalog_projects_a_queued_employees_shell() {
    use waku_protocol::boss::{BossResult, EmployeeLifecycle};
    let root = std::env::temp_dir().join(format!("summon-catalog-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    let (_held_task, _held) = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    let BossResult::Summoned {
        session_id, state, ..
    } = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "queued job",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .expect("at capacity the summon must still be accepted")
    else {
        panic!("expected a summoned result")
    };
    assert_eq!(state, EmployeeLifecycle::Queued);

    // A client-side draft has no business in the catalog — push one
    // straight into the store so the projection must prove it skipped
    // the unmanaged row while keeping the employee shell.
    let mut draft = AgentSession::new(
        backend.task_state.lock().sessions[0].project_id,
        ProviderKind::Codex,
    );
    draft.id = Uuid::new_v4();
    let draft_id = draft.id;
    {
        let mut state = backend.task_state.lock();
        state.push_session(draft);
        backend.task_store.save(&mut state).unwrap();
    }

    let response = backend
        .handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::LoadTaskState,
            },
            EventSink::detached(),
            None,
        )
        .unwrap();
    let ResponsePayload::TaskState { sessions, .. } = response else {
        panic!("expected catalog")
    };
    let shell = sessions
        .iter()
        .find(|session| session.id == session_id)
        .expect("a queued employee's shell belongs in the catalog");
    assert!(!shell.detail_loaded);
    assert!(
        !sessions.iter().any(|session| session.id == draft_id),
        "an unmanaged unstarted draft must not project"
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// Same-priority queued tickets attempt in sequence order, which the
/// broker ledger's grant order records directly.
#[test]
fn queued_tickets_dispatch_fifo_once_capacity_frees() {
    use waku_protocol::boss::{BossResult, EmployeeLifecycle};
    let root = std::env::temp_dir().join(format!("summon-fifo-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    let (held_task, held) = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    let summon = |job: &str| {
        let BossResult::Summoned {
            session_id, state, ..
        } = backend
            .handle_boss_operation(
                Some(boss),
                summon_op(&backend, &root, job, ProviderKind::Codex, Some("gpt-5.5")),
                &EventSink::detached(),
            )
            .unwrap()
        else {
            panic!("expected a summoned result")
        };
        assert_eq!(state, EmployeeLifecycle::Queued);
        session_id
    };
    let first = summon("first");
    let second = summon("second");
    assert_eq!(
        backend.boss.queue_position(second),
        Some(2),
        "the second ticket waits behind the first"
    );
    // Both queued tickets are considered in admission order.
    let heads: Vec<Uuid> = backend
        .boss
        .queued_heads()
        .iter()
        .map(|entry| entry.session_id)
        .collect();
    assert_eq!(heads, vec![first, second]);

    backend
        .resource_broker()
        .unwrap()
        .release_admission(held_task, held);
    backend.run_summon_scheduler();
    // With the pool free both dispatched in turn and expired on the
    // missing binary.
    assert!(backend.boss.employee(first).unwrap().expired);
    assert!(backend.boss.employee(second).unwrap().expired);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `setResources` rewrites a queued ticket in place: the next
/// admission attempt must satisfy the declared host set, so a ticket
/// that would have dispatched on the freed model slot keeps waiting
/// until the claimed capacity is also free.
#[test]
fn set_resources_on_a_queued_ticket_changes_its_admission() {
    use waku_protocol::boss::{
        AdmissionBlocker, BossOperation, BossResult, EmployeeControl, EmployeeLifecycle,
    };
    use waku_protocol::resources::{ResourceOperation, ResourceSet};
    let root = std::env::temp_dir().join(format!("summon-setres-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    let (held_task, held) = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");
    // Occupy the host's single native-build slot so the edited set
    // cannot grant the moment the model pool frees.
    let build_holder = Uuid::new_v4();
    let held_build = backend
        .resource_broker()
        .unwrap()
        .operate(
            build_holder,
            ResourceOperation::Acquire {
                resources: ResourceSet {
                    native_builds: 1,
                    ..Default::default()
                },
                purpose: "fixture build".into(),
                holder_pid: std::process::id(),
                wait_seconds: 60,
                parent: None,
            },
        )
        .unwrap()
        .request_id
        .unwrap();

    let BossResult::Summoned {
        session_id, state, ..
    } = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "resourced job",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    assert_eq!(state, EmployeeLifecycle::Queued);

    let result = backend
        .handle_boss_operation(
            Some(boss),
            BossOperation::Control {
                session_id,
                action: EmployeeControl::SetResources {
                    resources: ResourceSet {
                        native_builds: 1,
                        ..Default::default()
                    },
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert!(matches!(result, BossResult::Saved));
    let ticket = backend.boss.employee(session_id).unwrap().ticket.unwrap();
    assert_eq!(ticket.resources.native_builds, 1);

    // The freed model slot no longer suffices — admission now also
    // waits on the claimed build capacity.
    backend
        .resource_broker()
        .unwrap()
        .release_admission(held_task, held);
    backend.run_summon_scheduler();
    let employee = backend.boss.employee(session_id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
    let ticket = employee.ticket.clone().unwrap();
    assert!(
        ticket
            .blocked_by
            .iter()
            .any(|blocker| matches!(blocker, AdmissionBlocker::HostResources { .. })),
        "expected host capacity as the wait reason: {:?}",
        ticket.blocked_by
    );

    // Once the host set frees, admission proceeds — the fixture's
    // missing binary expires the launch like any dispatch.
    backend
        .resource_broker()
        .unwrap()
        .operate(build_holder, ResourceOperation::Release { id: held_build })
        .unwrap();
    backend.run_summon_scheduler();
    assert!(backend.boss.employee(session_id).unwrap().expired);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `setResources` on a working employee parks the new set on its
/// ticket: the record never leaves `working`, the held claims stand
/// until the new set grants, and the swap then releases the old
/// reservation atomically.
#[test]
fn set_resources_on_a_working_employee_swaps_in_place() {
    use waku_protocol::boss::{
        AdmissionBlocker, BossOperation, BossResult, EmployeeControl, EmployeeLifecycle,
    };
    use waku_protocol::resources::{AdmissionClaim, ResourceOperation, ResourceSet};
    let root = std::env::temp_dir().join(format!("summon-swap-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 2);
    let _fixture_slot = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    // Walk a queued ticket to `working` the way the dispatch
    // notification fixture does — grant its reservation id by hand,
    // then settle the durable transitions. `allowBurst` is set on the
    // ticket so the parked update's claim re-qualifies under the
    // burst half of the pool the fixture occupies.
    let BossResult::Summoned {
        session_id, state, ..
    } = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "swap job",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    assert_eq!(state, EmployeeLifecycle::Queued);
    assert!(
        backend
            .boss
            .reticket(session_id, |ticket| ticket.allow_burst = true)
            .unwrap()
    );
    let reservation = Uuid::from_u128(session_id.as_u128() ^ 1);
    let attempt = backend
        .resource_broker()
        .unwrap()
        .try_admission(
            session_id,
            reservation,
            ResourceSet::default(),
            "summon dispatch".into(),
            AdmissionClaim {
                daemon: backend.boss.document().identity.id,
                provider: ProviderKind::Codex.id().into(),
                model: "gpt-5.5".into(),
                // The fixture's slot is held — this grant spends the
                // pool's burst half.
                live_limit: 1,
                hard_cap: 2,
                allow_burst: true,
            },
        )
        .unwrap();
    assert!(attempt.granted);
    assert!(
        backend
            .boss
            .mark_dispatching(session_id, 1, Some(reservation))
            .unwrap()
    );
    assert!(backend.boss.mark_working(session_id, 1).unwrap());

    // Occupy the native-build slot so the parked update must wait.
    let build_holder = Uuid::new_v4();
    let held_build = backend
        .resource_broker()
        .unwrap()
        .operate(
            build_holder,
            ResourceOperation::Acquire {
                resources: ResourceSet {
                    native_builds: 1,
                    ..Default::default()
                },
                purpose: "fixture build".into(),
                holder_pid: std::process::id(),
                wait_seconds: 60,
                parent: None,
            },
        )
        .unwrap()
        .request_id
        .unwrap();

    let result = backend
        .handle_boss_operation(
            Some(boss),
            BossOperation::Control {
                session_id,
                action: EmployeeControl::SetResources {
                    resources: ResourceSet {
                        native_builds: 1,
                        ..Default::default()
                    },
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert!(matches!(result, BossResult::Saved));

    // Nothing was re-admitted or torn down: still working, still the
    // old reservation and the old zero set — the parked update only
    // records intent plus its wait reason.
    let employee = backend.boss.employee(session_id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Working);
    let ticket = employee.ticket.clone().unwrap();
    assert_eq!(ticket.reservation, Some(reservation));
    assert_eq!(ticket.resources, ResourceSet::default());
    let pending_id = ticket.pending_reservation.expect("the update parked");
    assert_ne!(pending_id, reservation);
    assert_eq!(
        ticket
            .pending_resources
            .as_ref()
            .map(|set| set.native_builds),
        Some(1)
    );
    assert!(
        ticket
            .blocked_by
            .iter()
            .any(|blocker| matches!(blocker, AdmissionBlocker::HostResources { .. })),
        "the parked update carries its wait reason: {:?}",
        ticket.blocked_by
    );

    // Capacity frees: the next pass grants the new set, swaps the
    // ticket onto it, and releases the old claims — all while the
    // record never left `working`.
    backend
        .resource_broker()
        .unwrap()
        .operate(build_holder, ResourceOperation::Release { id: held_build })
        .unwrap();
    backend.run_summon_scheduler();
    let employee = backend.boss.employee(session_id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Working);
    let ticket = employee.ticket.clone().unwrap();
    assert_eq!(ticket.reservation, Some(pending_id));
    assert_eq!(ticket.resources.native_builds, 1);
    assert!(ticket.pending_resources.is_none());
    assert!(ticket.pending_reservation.is_none());
    assert!(ticket.blocked_by.is_empty());

    // The ledger agrees: the swapped-in reservation holds the new
    // set and the old one's claims are gone.
    let status = backend
        .resource_broker()
        .unwrap()
        .operate(Uuid::new_v4(), ResourceOperation::Status { id: None })
        .unwrap();
    let held: Vec<_> = status
        .reservations
        .iter()
        .filter(|r| r.task == session_id && r.granted_at.is_some() && !r.released)
        .collect();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].id, pending_id);
    assert_eq!(held[0].resources.native_builds, 1);

    // Naming the held set again parks nothing — it is already the
    // admission's set.
    backend
        .handle_boss_operation(
            Some(boss),
            BossOperation::Control {
                session_id,
                action: EmployeeControl::SetResources {
                    resources: ResourceSet {
                        native_builds: 1,
                        ..Default::default()
                    },
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    let ticket = backend.boss.employee(session_id).unwrap().ticket.unwrap();
    assert!(ticket.pending_reservation.is_none());
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A full model lane does not block a later ticket for a free model in
/// the same supervisor queue. The skipped ticket keeps its own reason.
#[test]
fn blocked_queue_ticket_does_not_stall_a_free_model_lane() {
    use waku_protocol::boss::{AdmissionBlocker, BossResult, EmployeeLifecycle};
    let root = std::env::temp_dir().join(format!("summon-lane-skip-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Devin, "swe-2", 1, 1);
    let (_held_task, _held) = hold_model_slot(&backend, ProviderKind::Devin, "swe-2");

    let summon = |job: &str, provider, model: &str| {
        let BossResult::Summoned { session_id, .. } = backend
            .handle_boss_operation(
                Some(boss),
                summon_op(&backend, &root, job, provider, Some(model)),
                &EventSink::detached(),
            )
            .unwrap()
        else {
            panic!("expected a summoned result")
        };
        session_id
    };
    let blocked = summon("blocked swe", ProviderKind::Devin, "swe-2");
    // The free model is admitted immediately, then the missing Codex
    // CLI makes launch fail. Its durable employee record proves it was
    // attempted despite the earlier blocked ticket.
    assert!(
        backend
            .handle_boss_operation(
                Some(boss),
                summon_op(
                    &backend,
                    &root,
                    "free luna",
                    ProviderKind::Codex,
                    Some("gpt-6-luna")
                ),
                &EventSink::detached(),
            )
            .is_err()
    );
    let later = backend
        .boss
        .document()
        .employees
        .into_iter()
        .find(|entry| entry.job_title == "free luna")
        .unwrap()
        .session_id;

    let blocked_employee = backend.boss.employee(blocked).unwrap();
    assert_eq!(blocked_employee.lifecycle(), EmployeeLifecycle::Queued);
    assert!(
        blocked_employee
            .ticket
            .as_ref()
            .unwrap()
            .blocked_by
            .iter()
            .any(|reason| matches!(reason, AdmissionBlocker::ModelLimit { used: 1, limit: 1 }))
    );
    assert!(backend.boss.employee(later).unwrap().expired);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// Model caps key on the provider+model pair: a full codex pool does
/// not block a claude ticket.
#[test]
fn model_limits_key_on_the_provider_model_pair() {
    use waku_protocol::boss::{BossOperation, EmployeeLifecycle};
    let root = std::env::temp_dir().join(format!("summon-keys-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);

    let blocked = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "codex job",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap();
    let waku_protocol::boss::BossResult::Summoned {
        session_id,
        state,
        admission,
        ..
    } = blocked
    else {
        panic!("expected a summoned result")
    };
    assert_eq!(state, EmployeeLifecycle::Queued);
    assert!(admission.unwrap().blocked_by.iter().any(|blocker| matches!(
        blocker,
        waku_protocol::boss::AdmissionBlocker::ModelLimit { used: 0, limit: 0 }
    )));

    // The distinct free model is considered immediately and its
    // launch fails only because Claude Code is not installed.
    assert!(
        backend
            .handle_boss_operation(
                Some(boss),
                summon_op(&backend, &root, "claude job", ProviderKind::Claude, None),
                &EventSink::detached(),
            )
            .is_err()
    );
    let claude_id = backend
        .boss
        .document()
        .employees
        .into_iter()
        .find(|entry| entry.job_title == "claude job")
        .unwrap()
        .session_id;
    assert!(backend.boss.employee(claude_id).unwrap().expired);

    // The blocked Codex ticket remains queued until explicitly stopped.
    backend
        .handle_boss_operation(
            Some(boss),
            BossOperation::Control {
                session_id,
                action: waku_protocol::boss::EmployeeControl::Stop,
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert!(backend.boss.employee(claude_id).unwrap().expired);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `liveLimit` is the normal pool and `hardCap` opens only to summons
/// that explicitly ask for burst — and stops there.
#[test]
fn burst_admission_needs_the_flag_and_stops_at_hard_cap() {
    use waku_protocol::boss::{AdmissionBlocker, BossOperation, BossResult, EmployeeLifecycle};
    let root = std::env::temp_dir().join(format!("summon-burst-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 2);
    hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");
    let (held_task, held) = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    // The burst-flagged ticket is the head: its pool is the hard cap —
    // still full at two held slots.
    let mut op = summon_op(
        &backend,
        &root,
        "burst",
        ProviderKind::Codex,
        Some("gpt-5.5"),
    );
    if let BossOperation::Summon { allow_burst, .. } = &mut op {
        *allow_burst = true;
    }
    let BossResult::Summoned {
        session_id: burst_id,
        state,
        admission,
        ..
    } = backend
        .handle_boss_operation(Some(boss), op, &EventSink::detached())
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    assert_eq!(state, EmployeeLifecycle::Queued);
    assert!(
        admission
            .unwrap()
            .blocked_by
            .iter()
            .any(|blocker| matches!(blocker, AdmissionBlocker::ModelLimit { used: 2, limit: 2 }))
    );
    // Without the flag the ticket reports its own live cap, even
    // while the earlier burst ticket remains blocked at the hard cap.
    let waku_protocol::boss::BossResult::Summoned { admission, .. } = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "plain",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    assert!(
        admission
            .unwrap()
            .blocked_by
            .iter()
            .any(|blocker| matches!(blocker, AdmissionBlocker::ModelLimit { used: 2, limit: 1 }))
    );

    // Free one slot: the burst head grants into the hard-cap half and
    // expires on launch; the plain ticket then reads its own live cap.
    backend
        .resource_broker()
        .unwrap()
        .release_admission(held_task, held);
    backend.run_summon_scheduler();
    assert!(backend.boss.employee(burst_id).unwrap().expired);
    let plain = backend.boss.queued_heads();
    assert_eq!(plain.len(), 1);
    assert!(
        plain[0]
            .ticket
            .as_ref()
            .unwrap()
            .blocked_by
            .iter()
            .any(|blocker| matches!(blocker, AdmissionBlocker::ModelLimit { used: 1, limit: 1 })),
        "the non-burst head reports the live pool: {:?}",
        plain[0].ticket.as_ref().unwrap().blocked_by
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// Queued employees take prompts into their durable envelope and stop
/// settles them without ever touching a reservation.
#[test]
fn queued_employees_take_prompts_and_expire_without_claims() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let root = std::env::temp_dir().join(format!("summon-control-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    let waku_protocol::boss::BossResult::Summoned { session_id, .. } = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "parked",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    let control = |action: EmployeeControl| {
        backend.handle_boss_operation(
            Some(boss),
            BossOperation::Control { session_id, action },
            &EventSink::detached(),
        )
    };
    control(EmployeeControl::Prompt {
        prompt: "also check the migrations".into(),
        delivery: Some(AgentPromptDelivery::Interrupt),
    })
    .unwrap();
    let employee = backend.boss.employee(session_id).unwrap();
    assert_eq!(
        employee.ticket.as_ref().unwrap().pending_prompts,
        vec!["also check the migrations".to_owned()]
    );
    assert!(
        control(EmployeeControl::Steer {
            prompt: "nope".into(),
            job_title: None,
        })
        .is_err()
    );
    backend
        .boss
        .set_employee_goal(session_id, waku_protocol::boss::EmployeeGoal::Errand)
        .unwrap();
    control(EmployeeControl::Stop).unwrap();
    let employee = backend.boss.employee(session_id).unwrap();
    assert!(employee.expired);
    assert!(employee.cancelled);
    assert!(
        !backend.agent.has_queued(boss),
        "cancelling queued work must not enqueue a supervisor finish report"
    );
    let state = backend.task_state.lock();
    let supervisor = state
        .sessions
        .iter()
        .find(|session| session.id == boss)
        .unwrap();
    assert!(
        supervisor
            .messages
            .iter()
            .all(|message| message.report_trigger.is_none())
            && supervisor.queued_messages.is_empty(),
        "cancelling queued work must not start or mirror a supervisor finish report"
    );
    drop(state);
    assert!(backend.sessions.lock().get(&session_id).is_none());
    let status = backend
        .resource_broker()
        .unwrap()
        .operate(
            Uuid::new_v4(),
            waku_protocol::resources::ResourceOperation::Status { id: None },
        )
        .unwrap();
    assert_eq!(
        status
            .reservations
            .iter()
            .filter(|reservation| !reservation.released)
            .count(),
        1,
        "only the fixture slot remains — the cancelled ticket held nothing"
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A queued `setModel` retickets into the destination pool: the
/// sequence is kept, and when the destination is itself full the
/// ticket waits under the new key.
#[test]
fn setmodel_on_a_queued_ticket_moves_its_admission_key() {
    use waku_protocol::boss::{
        AdmissionBlocker, BossOperation, EmployeeControl, EmployeeLifecycle,
    };
    let root = std::env::temp_dir().join(format!("summon-setmodel-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    let waku_protocol::boss::BossResult::Summoned { session_id, .. } = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "rekey",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    let sequence = backend
        .boss
        .employee(session_id)
        .and_then(|employee| employee.ticket.map(|ticket| ticket.sequence))
        .unwrap();

    // Fill the claude pool too, so the reticket has somewhere
    // provably different to land.
    let claude_selection = backend
        .resolve_agent_task_selection(
            None,
            &AgentCreateSelection {
                provider: Some(ProviderKind::Claude),
                model: None,
                title: None,
                reasoning_effort: None,
                service_tier: None,
                context_window: None,
            },
            &root.join("repo"),
            "",
            false,
        )
        .unwrap();
    let claude_model = claude_selection
        .model
        .clone()
        .or_else(|| claude_selection.concrete_model.clone())
        .unwrap();
    hold_model_slot(&backend, ProviderKind::Claude, &claude_model);
    set_model_policy(&backend, ProviderKind::Claude, &claude_model, 1, 1);

    backend
        .handle_boss_operation(
            Some(boss),
            BossOperation::Control {
                session_id,
                action: EmployeeControl::SetModel {
                    provider: ProviderKind::Claude,
                    model: claude_model.clone(),
                    reasoning_effort: None,
                    interrupt: None,
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    let employee = backend.boss.employee(session_id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
    let ticket = employee.ticket.as_ref().unwrap();
    assert_eq!(ticket.provider, ProviderKind::Claude);
    assert_eq!(ticket.model, claude_model);
    assert_eq!(ticket.sequence, sequence, "retickets keep their position");
    assert!(
        ticket
            .blocked_by
            .iter()
            .any(|blocker| matches!(blocker, AdmissionBlocker::ModelLimit { used: 1, limit: 1 })),
        "the destination pool reports its own wait reason"
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A prompt to a finished employee re-enters admission against
/// current policy — a lowered cap keeps the revival queued rather
/// than resurrecting around the pool.
#[test]
fn an_expired_employees_prompt_requeues_for_admission() {
    use waku_protocol::boss::{BossOperation, EmployeeControl, EmployeeLifecycle};
    let root = std::env::temp_dir().join(format!("summon-revive-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);

    // The first summon grants and fails at launch — expired record.
    let error = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "revive",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("Employee launch failed"));
    let session_id = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|session| session.id != boss)
        .unwrap()
        .id;
    assert!(backend.boss.employee(session_id).unwrap().expired);

    // Close the pool, then prompt the expired employee — it queues as
    // generation two with the prompt as its dispatch envelope.
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    backend
        .handle_boss_operation(
            Some(boss),
            BossOperation::Control {
                session_id,
                action: EmployeeControl::Prompt {
                    prompt: "try again".into(),
                    delivery: Some(AgentPromptDelivery::Interrupt),
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    let employee = backend.boss.employee(session_id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
    assert!(!employee.expired);
    let ticket = employee.ticket.as_ref().unwrap();
    assert_eq!(ticket.generation, 2);
    // The session already ran — the revive prompt replays as a turn,
    // not as a fresh envelope.
    assert_eq!(ticket.pending_prompts, vec!["try again".to_owned()]);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `resume` re-admits the expired ticket through the ordinary queue:
/// the synthesized prompt names the interruption it answers, the
/// ticket counts the resume against that cause, and the record's
/// expiry clears with the rest of the old admission's flags.
#[test]
fn a_resume_requeues_the_expired_employee_with_its_cause() {
    use waku_protocol::boss::{BossOperation, EmployeeLifecycle, ExpiryCause};
    let root = std::env::temp_dir().join(format!("boss-resume-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);

    // The first summon grants and fails at launch — expired record
    // classified `failed`.
    backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "revive",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap_err();
    let session_id = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|session| session.id != boss)
        .unwrap()
        .id;
    assert_eq!(
        backend
            .boss
            .employee(session_id)
            .unwrap()
            .expiry
            .unwrap()
            .cause,
        ExpiryCause::Failed
    );

    // Close the pool so the resume waits in the queue where the
    // ticket's adjustments are readable.
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    backend
        .handle_boss_operation(
            Some(boss),
            BossOperation::Resume { session_id },
            &EventSink::detached(),
        )
        .unwrap();
    let employee = backend.boss.employee(session_id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
    assert!(!employee.expired);
    assert!(employee.expiry.is_none());
    let ticket = employee.ticket.as_ref().unwrap();
    assert_eq!(ticket.generation, 2);
    assert_eq!(ticket.resume_count, 1);
    assert_eq!(ticket.last_resumed_cause, Some(ExpiryCause::Failed));
    assert_eq!(ticket.interruptions.len(), 1);
    let prompt = ticket.pending_prompts.last().unwrap();
    assert!(prompt.contains("interrupted"), "{prompt}");
    assert!(prompt.contains("failed turn"), "{prompt}");
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// Resume is for expired records — a queued or working employee takes
/// its next input as a prompt, not a revive.
#[test]
fn a_resume_refuses_a_live_employee() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("boss-resume-live-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "still queued",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap();
    let session_id = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|session| session.id != boss)
        .unwrap()
        .id;
    let error = backend
        .handle_boss_operation(
            Some(boss),
            BossOperation::Resume { session_id },
            &EventSink::detached(),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("still live"), "{error:#}");
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// The adopted-worktree refusal survives resume: a finished employee
/// whose checkout a later summon owns comes back as an error, not a
/// resume into somebody else's worktree.
#[test]
fn a_resume_refuses_an_adopted_worktree() {
    use waku_protocol::boss::{BossOperation, EmployeeSettle};
    let root = std::env::temp_dir().join(format!("boss-resume-adopted-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, _parent, _child) = employee_finish_fixture(&root);
    {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == employee_id)
            .unwrap();
        backend.task_store.hydrate(session).unwrap();
        session.workspace = crate::model::SessionWorkspace::Worktree {
            path: root.join("worktree"),
            name: "worktree".into(),
            branch: Some("wip".into()),
            base_branch: Some("main".into()),
            adopted_by: Some(Uuid::new_v4()),
        };
        backend.task_store.save(&mut state).unwrap();
    }
    backend
        .finish_boss_employee(employee_id, false, EmployeeSettle::Restarted)
        .unwrap();
    let error = backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Resume {
                session_id: employee_id,
            },
            &EventSink::detached(),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("adopted"), "{error:#}");
    let _ = std::fs::remove_dir_all(root);
}

/// A resume keeps prompts parked at expiry ahead of its continuation
/// note — the ordinary ticket merge does the ordering.
#[test]
fn a_resume_drains_parked_prompts_first() {
    use waku_protocol::boss::{BossOperation, EmployeeControl, EmployeeLifecycle, EmployeeSettle};
    let root = std::env::temp_dir().join(format!("boss-resume-parked-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, _parent, _child) = employee_finish_fixture(&root);
    // Pin a model so the synthesized ticket takes a pool the test
    // can close.
    {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == employee_id)
            .unwrap();
        backend.task_store.hydrate(session).unwrap();
        session.model = Some("gpt-5.5".into());
        backend.task_store.save(&mut state).unwrap();
    }
    backend
        .agent
        .note_driver_event(employee_id, &DriverEvent::TurnStarted);
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::Prompt {
                    prompt: "queued while working".into(),
                    delivery: Some(crate::protocol::AgentPromptDelivery::Queue),
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    backend
        .finish_boss_employee(employee_id, false, EmployeeSettle::Restarted)
        .unwrap();
    // Close the pool so the resume queues instead of dispatching —
    // the parked prompt and the resume note read off the ticket.
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Resume {
                session_id: employee_id,
            },
            &EventSink::detached(),
        )
        .unwrap();
    let employee = backend.boss.employee(employee_id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
    let pending = &employee.ticket.as_ref().unwrap().pending_prompts;
    assert_eq!(pending.len(), 2, "{pending:?}");
    assert_eq!(pending[0], "queued while working");
    assert!(pending[1].contains("interrupted"), "{}", pending[1]);
    let _ = std::fs::remove_dir_all(root);
}

/// A parked prompt is unfinished work, not a finish: a `TurnFinished`
/// arriving with the queue still full drains the entry into a fresh
/// prompt and the employee stays live. Only the next finish — queue
/// empty — settles the record.
#[test]
fn an_employee_with_parked_prompts_drains_instead_of_expiring() {
    use waku_protocol::boss::{BossOperation, EmployeeControl, ExpiryCause};
    let root = std::env::temp_dir().join(format!("boss-drain-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, _parent, child_capture) = employee_finish_fixture(&root);
    // The settle's finish rides the callback `start_automations` binds
    // in production — wire it directly so `note_settled` expires.
    let backend = Arc::new(backend);
    backend.bind_boss_finish_callback();
    let runtime_id = backend
        .sessions
        .lock()
        .get(&employee_id)
        .unwrap()
        .runtime_id;
    // The follow-up lands mid-turn, so it parks in the prompt queue.
    backend
        .agent
        .note_driver_event(employee_id, &DriverEvent::TurnStarted);
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::Prompt {
                    prompt: "queued while working".into(),
                    delivery: Some(crate::protocol::AgentPromptDelivery::Queue),
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert!(backend.agent.has_queued(employee_id));

    let forward = |feed: &[DriverEvent]| {
        let events = EventSink::detached().begin_session_runtime(employee_id, runtime_id);
        let (driver_events, event_receiver) = driver::test_event_channel();
        for event in feed {
            driver_events.send(event.clone()).unwrap();
        }
        drop(driver_events);
        forward_driver_events(
            employee_id,
            runtime_id,
            event_receiver,
            events,
            DriverHandle::from_control(child_capture.clone()),
            backend.agent.clone(),
            backend.task_state.clone(),
            backend.task_store.clone(),
            backend.sessions.clone(),
            backend.automations.clone(),
            backend.boss.clone(),
            backend.auto_prompts.clone(),
            backend.repo_maps.clone(),
        );
    };
    forward(&[DriverEvent::TurnFinished {
        success: true,
        summary: None,
        summary_i18n: None,
    }]);

    // The parked prompt delivered to the live employee — no settle,
    // no expiry, nothing left parked.
    let employee = backend.boss.employee(employee_id).unwrap();
    assert!(!employee.expired);
    assert!(!backend.agent.has_queued(employee_id));
    assert!(
        child_capture
            .prompts
            .lock()
            .iter()
            .any(|prompt| prompt.contains("queued while working")),
        "{:?}",
        child_capture.prompts.lock()
    );

    // Queue empty now — the next finish is a real settle. The expiry
    // itself runs off the forwarder on the finish callback's thread.
    forward(&[
        DriverEvent::TurnStarted,
        DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None,
        },
    ]);
    let expired = (0..400).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(5));
        backend
            .boss
            .employee(employee_id)
            .is_some_and(|employee| employee.expired)
    });
    assert!(expired, "the empty-queue settle expired the employee");
    let employee = backend.boss.employee(employee_id).unwrap();
    assert_eq!(
        employee.expiry.as_ref().unwrap().cause,
        ExpiryCause::Finished
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// An expiry that lands mid-turn does not read as a user's stop: the
/// turn record attributes the interruption to the daemon and the
/// transcript carries the "Turn interrupted — …" row naming the cause.
#[test]
fn an_interrupted_expiry_attributes_the_turn_and_names_the_cause() {
    use waku_protocol::boss::EmployeeSettle;
    use waku_protocol::model::{TranscriptNotice, TranscriptNoticeStatus, TurnInterruption};
    let root = std::env::temp_dir().join(format!("boss-interrupt-{}", Uuid::new_v4()));
    let (backend, _supervisor, employee_id, _parent, _child) = employee_finish_fixture(&root);
    // Reopen a turn so the restart settle lands mid-work.
    {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == employee_id)
            .unwrap();
        backend.task_store.hydrate(session).unwrap();
        session.begin_turn("still running when the daemon restarted");
        backend.task_store.save(&mut state).unwrap();
    }
    backend
        .finish_boss_employee(employee_id, false, EmployeeSettle::Restarted)
        .unwrap();
    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == employee_id)
        .unwrap();
    let turn = session.turns.last().unwrap();
    assert_eq!(turn.status, TurnStatus::Interrupted);
    assert_eq!(turn.interruption, Some(TurnInterruption::Daemon));
    let notice = session
        .messages
        .iter()
        .find(|message| {
            matches!(
                &message.notice,
                Some(TranscriptNotice::Status {
                    kind: TranscriptNoticeStatus::Interrupted,
                })
            )
        })
        .expect("the interruption wrote a notice row");
    assert!(
        notice.content.contains("daemon restarted"),
        "{}",
        notice.content
    );
    drop(state);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A steer carrying `jobTitle` retitles the job as it requeues —
/// bookkeeping on the roster record, not a queued prompt; a steer
/// without one leaves the label alone.
#[test]
fn a_redirecting_steer_retitles_the_resumed_employee() {
    use waku_protocol::boss::{BossOperation, EmployeeControl, EmployeeLifecycle};
    let root = std::env::temp_dir().join(format!("steer-retitle-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);

    // The first summon grants and fails at launch — expired record.
    backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "revive",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap_err();
    let session_id = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|session| session.id != boss)
        .unwrap()
        .id;
    assert_eq!(
        backend.boss.employee(session_id).unwrap().job_title,
        "revive"
    );

    // Close the pool, then steer the expired employee with a new
    // title — it requeues with the steer as a parked prompt and the
    // roster record relabels.
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    let control = |action: EmployeeControl| {
        backend.handle_boss_operation(
            Some(boss),
            BossOperation::Control { session_id, action },
            &EventSink::detached(),
        )
    };
    control(EmployeeControl::Steer {
        prompt: "redirect: review the patch instead".into(),
        job_title: Some("Patch review".into()),
    })
    .unwrap();
    let employee = backend.boss.employee(session_id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
    assert_eq!(employee.job_title, "Patch review");
    assert_eq!(
        employee.ticket.as_ref().unwrap().pending_prompts,
        vec!["redirect: review the patch instead".to_owned()]
    );

    // Expire it again and steer without a title — the label stays.
    control(EmployeeControl::Stop).unwrap();
    control(EmployeeControl::Steer {
        prompt: "one more pass".into(),
        job_title: None,
    })
    .unwrap();
    let employee = backend.boss.employee(session_id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
    assert_eq!(employee.job_title, "Patch review");

    // An empty title is rejected — the steer never queued.
    control(EmployeeControl::Stop).unwrap();
    assert!(
        control(EmployeeControl::Steer {
            prompt: "bad retitle".into(),
            job_title: Some("   ".into()),
        })
        .is_err()
    );
    let employee = backend.boss.employee(session_id).unwrap();
    assert_eq!(employee.job_title, "Patch review");
    assert_eq!(
        employee.ticket.as_ref().unwrap().pending_prompts,
        vec![
            "redirect: review the patch instead".to_owned(),
            "one more pass".to_owned()
        ]
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `requestId` dedupes retries: same request returns the same
/// employee; a mutated request with the id is rejected.
#[test]
fn a_request_id_retries_the_same_admission() {
    use waku_protocol::boss::{BossOperation, EmployeeLifecycle};
    let root = std::env::temp_dir().join(format!("summon-dedupe-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    let request_id = Uuid::new_v4();
    let mut op = summon_op(
        &backend,
        &root,
        "first",
        ProviderKind::Codex,
        Some("gpt-5.5"),
    );
    if let BossOperation::Summon {
        request_id: slot, ..
    } = &mut op
    {
        *slot = Some(request_id);
    }
    let waku_protocol::boss::BossResult::Summoned {
        session_id, state, ..
    } = backend
        .handle_boss_operation(Some(boss), op.clone(), &EventSink::detached())
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    assert_eq!(state, EmployeeLifecycle::Queued);

    // Same request id + same payload: the retry returns the ticket,
    // not a second employee.
    let waku_protocol::boss::BossResult::Summoned {
        session_id: retried,
        ..
    } = backend
        .handle_boss_operation(Some(boss), op, &EventSink::detached())
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    assert_eq!(retried, session_id);
    assert_eq!(backend.boss.document().employees.len(), 1);

    // Same id, different ask: a genuine mismatch errors.
    let mut altered = summon_op(
        &backend,
        &root,
        "different",
        ProviderKind::Codex,
        Some("gpt-5.5"),
    );
    if let BossOperation::Summon {
        request_id: slot, ..
    } = &mut altered
    {
        *slot = Some(request_id);
    }
    assert!(
        backend
            .handle_boss_operation(Some(boss), altered, &EventSink::detached())
            .is_err()
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// Durable tickets survive a daemon restart: the reopened document
/// still queues the employee (it never lands on the interrupted
/// list), with the goal, group, priority, and parked prompts intact.
#[test]
fn queued_tickets_survive_restart_with_their_goals() {
    use waku_protocol::boss::{BossOperation, EmployeeControl, EmployeeLifecycle};
    let root = std::env::temp_dir().join(format!("summon-restart-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    let goal = Uuid::new_v4();
    let mut op = summon_op(
        &backend,
        &root,
        "restart",
        ProviderKind::Codex,
        Some("gpt-5.5"),
    );
    if let BossOperation::Summon {
        goal_id,
        group_id,
        priority,
        ..
    } = &mut op
    {
        *goal_id = Some(goal);
        *group_id = Some("wave-1".into());
        *priority = Some(5);
    }
    let waku_protocol::boss::BossResult::Summoned { session_id, .. } = backend
        .handle_boss_operation(Some(boss), op, &EventSink::detached())
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    backend
        .handle_boss_operation(
            Some(boss),
            BossOperation::Control {
                session_id,
                action: EmployeeControl::Prompt {
                    prompt: "queued follow-up".into(),
                    delivery: Some(AgentPromptDelivery::Interrupt),
                },
            },
            &EventSink::detached(),
        )
        .unwrap();

    let reopened = crate::boss::BossService::open(root.join("boss")).unwrap();
    let employee = reopened.employee(session_id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
    assert!(!employee.expired);
    let ticket = employee.ticket.as_ref().unwrap();
    assert_eq!(ticket.goal_id, Some(goal));
    assert_eq!(ticket.group_id.as_deref(), Some("wave-1"));
    assert_eq!(ticket.priority, Some(5));
    assert_eq!(ticket.pending_prompts, vec!["queued follow-up".to_owned()]);
    // The record is not interrupted — a queued ticket holds nothing
    // to clean up, so the roster reads it straight after reopening.
    assert!(reopened.require_active(session_id).is_ok());
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// Old dispatch outbox entries must not become supervisor prompts after
/// upgrading to silent dispatch.
#[test]
fn legacy_dispatch_notifications_are_retired_without_delivery() {
    use waku_protocol::boss::EmployeeLifecycle;
    let root = std::env::temp_dir().join(format!("summon-outbox-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);

    // Build a "working" employee without a real launch: queue the
    // ticket under a held slot, grant its reservation id, then walk
    // dispatching → working through the durable transitions.
    let (held_task, held) = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");
    let waku_protocol::boss::BossResult::Summoned { session_id, .. } = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "notify",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    let reservation = Uuid::from_u128(session_id.as_u128() ^ 1);
    let attempt = backend
        .resource_broker()
        .unwrap()
        .try_admission(
            session_id,
            reservation,
            waku_protocol::resources::ResourceSet::default(),
            "summon dispatch".into(),
            waku_protocol::resources::AdmissionClaim {
                daemon: backend.boss.document().identity.id,
                provider: ProviderKind::Codex.id().into(),
                model: "gpt-5.5".into(),
                // One slot is held — this grant goes into the burst
                // half of the pool.
                live_limit: 1,
                hard_cap: 2,
                allow_burst: true,
            },
        )
        .unwrap();
    assert!(attempt.granted);
    assert!(
        backend
            .boss
            .mark_dispatching(session_id, 1, Some(reservation))
            .unwrap()
    );
    assert!(backend.boss.mark_working(session_id, 1).unwrap());
    assert_eq!(
        backend.boss.employee_lifecycle(session_id),
        Some(EmployeeLifecycle::Working)
    );
    backend
        .boss
        .outbox_push(session_id, 1, ProviderKind::Codex, "gpt-5.5".into(), None)
        .unwrap();

    // Point the supervisor at a capture driver so deliveries land
    // somewhere countable.
    let capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        boss,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.to_path_buf(),
        },
    );
    backend.retire_dispatch_notifications();
    assert!(backend.boss.outbox_pending().is_empty());
    backend.retire_dispatch_notifications();
    assert!(capture.prompts.lock().is_empty());
    assert!(capture.steers.lock().is_empty());
    assert!(!backend.agent.has_queued(boss));
    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == boss)
        .unwrap();
    assert!(session.queued_messages.is_empty());
    assert!(
        !session
            .messages
            .iter()
            .any(|message| message.content.contains("has started working"))
    );
    drop(state);
    let _ = (held_task, held);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn employee_dispatch_is_silent_and_batches_queued_resumption() {
    use waku_protocol::boss::{BossOperation, BossResult, EmployeeControl};
    let root = std::env::temp_dir().join(format!("summon-wake-{}", Uuid::new_v4()));
    let (backend, supervisor) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    let mut operation = summon_op(
        &backend,
        &root,
        "wake",
        ProviderKind::Codex,
        Some("gpt-5.5"),
    );
    if let BossOperation::Summon { workspace, .. } = &mut operation {
        *workspace = Some(AgentWorkspace::Local);
    }
    let BossResult::Summoned { session_id, .. } = backend
        .handle_boss_operation(Some(supervisor), operation, &EventSink::detached())
        .unwrap()
    else {
        panic!("expected a summoned result")
    };
    let supervisor_capture = Arc::new(CaptureDriver::default());
    let initial_capture = Arc::new(CaptureDriver::default());
    for (id, capture) in [
        (supervisor, supervisor_capture.clone()),
        (session_id, initial_capture.clone()),
    ] {
        backend.sessions.lock().insert(
            id,
            RuntimeEntry {
                runtime_id: Uuid::new_v4(),
                driver: DriverHandle::from_control(capture),
                last_active: std::time::Instant::now(),
                resumable: false,
                computer_use_available: false,
                provider: ProviderKind::Codex,
                cwd: root.clone(),
            },
        );
    }
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    backend
        .dispatch_queued_head(&backend.boss.employee(session_id).unwrap())
        .unwrap();
    assert_eq!(initial_capture.prompts.lock().len(), 1);
    assert!(supervisor_capture.prompts.lock().is_empty());
    assert!(supervisor_capture.steers.lock().is_empty());
    assert!(!backend.agent.has_queued(supervisor));
    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == supervisor)
        .unwrap();
    assert!(session.queued_messages.is_empty());
    assert!(
        !session
            .messages
            .iter()
            .any(|message| message.content.contains("has started working"))
    );
    drop(state);
    backend
        .finish_boss_employee(
            session_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();

    let notices_after_finish = supervisor_capture.prompts.lock().len();
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    for prompt in ["First queued check", "Second queued check"] {
        backend
            .handle_boss_operation(
                Some(supervisor),
                BossOperation::Control {
                    session_id,
                    action: EmployeeControl::Prompt {
                        prompt: prompt.into(),
                        delivery: Some(AgentPromptDelivery::Queue),
                    },
                },
                &EventSink::detached(),
            )
            .unwrap();
    }
    let resumed_capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        session_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(resumed_capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    backend
        .dispatch_queued_head(&backend.boss.employee(session_id).unwrap())
        .unwrap();
    let prompts = resumed_capture.prompts.lock();
    assert_eq!(
        prompts.len(),
        1,
        "all parked prompts share one resumed turn"
    );
    assert!(prompts[0].contains("First queued check\n\nSecond queued check"));
    assert!(!backend.agent.has_queued(session_id));
    assert!(backend.boss.document().outbox.is_empty());
    assert_eq!(
        supervisor_capture.prompts.lock().len(),
        notices_after_finish,
        "resumption adds no notice; finish reports are still allowed"
    );
    drop(prompts);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// Two summons under one `groupId` make a wave: membership is
/// durable, every member's terminal state tallies once, and the
/// supervisor gets a single resolution notice whose outbox entry
/// dedupes re-delivery and survives reopening the document.
#[test]
fn a_wave_reports_once_when_every_member_settles() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("summon-wave-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    let (held_task, held) = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    let summon = |job: &str| {
        let mut op = summon_op(&backend, &root, job, ProviderKind::Codex, Some("gpt-5.5"));
        if let BossOperation::Summon { group_id, .. } = &mut op {
            *group_id = Some("wave-1".into());
        }
        let waku_protocol::boss::BossResult::Summoned { session_id, .. } = backend
            .handle_boss_operation(Some(boss), op, &EventSink::detached())
            .unwrap()
        else {
            panic!("expected a summoned result")
        };
        session_id
    };
    let first = summon("alpha");
    let second = summon("beta");
    let wave = backend
        .boss
        .document()
        .waves
        .iter()
        .find(|wave| wave.id == "wave-1")
        .expect("the group id opened a wave")
        .clone();
    assert_eq!(wave.supervisor_id, boss);
    assert_eq!(
        wave.members
            .iter()
            .map(|member| member.session_id)
            .collect::<Vec<_>>(),
        vec![first, second]
    );
    assert!(wave.resolved_at.is_none());

    // A capture driver counts what reaches the supervisor, then the
    // held slot frees: each member's launch dies on the fixture's
    // missing binary and expires as failed.
    let capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        boss,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.to_path_buf(),
        },
    );
    backend
        .resource_broker()
        .unwrap()
        .release_admission(held_task, held);
    backend.run_summon_scheduler();
    for member in [first, second] {
        assert!(backend.boss.employee(member).unwrap().expired);
    }

    let document = backend.boss.document();
    let wave = document
        .waves
        .iter()
        .find(|wave| wave.id == "wave-1")
        .unwrap();
    assert!(wave.resolved_at.is_some());
    assert_eq!(document.wave_outbox.len(), 1, "one resolution, one notice");
    let wave_prompts: Vec<String> = capture
        .prompts
        .lock()
        .iter()
        .filter(|prompt| prompt.contains("Wave \"wave-1\""))
        .cloned()
        .collect();
    assert_eq!(wave_prompts.len(), 1, "{wave_prompts:?}");
    assert!(wave_prompts[0].contains("0 finished, 2 blocked/failed"));
    assert!(backend.boss.wave_outbox_pending().is_empty());
    // Re-driving the outbox cannot repeat the parked notice.
    backend.deliver_wave_notifications();
    assert_eq!(
        capture
            .prompts
            .lock()
            .iter()
            .filter(|prompt| prompt.contains("Wave \"wave-1\""))
            .count(),
        1
    );
    // The resolution survives reopening the Boss document.
    let reopened = crate::boss::BossService::open(root.join("boss")).unwrap();
    let reopened_document = reopened.document();
    let wave = reopened_document
        .waves
        .iter()
        .find(|wave| wave.id == "wave-1")
        .unwrap();
    assert!(wave.resolved_at.is_some());
    assert_eq!(reopened_document.wave_outbox.len(), 1);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// Cancelling every member resolves the wave too — the notice reads
/// as cancelled, not a clean finish.
#[test]
fn an_all_cancelled_wave_reports_as_cancelled() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let root = std::env::temp_dir().join(format!("summon-wave-stop-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");

    let summon = |job: &str| {
        let mut op = summon_op(&backend, &root, job, ProviderKind::Codex, Some("gpt-5.5"));
        if let BossOperation::Summon { group_id, .. } = &mut op {
            *group_id = Some("wave-stop".into());
        }
        let waku_protocol::boss::BossResult::Summoned { session_id, .. } = backend
            .handle_boss_operation(Some(boss), op, &EventSink::detached())
            .unwrap()
        else {
            panic!("expected a summoned result")
        };
        session_id
    };
    let first = summon("alpha");
    let second = summon("beta");
    let capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        boss,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.to_path_buf(),
        },
    );
    for member in [first, second] {
        backend
            .handle_boss_operation(
                Some(boss),
                BossOperation::Control {
                    session_id: member,
                    action: EmployeeControl::Stop,
                },
                &EventSink::detached(),
            )
            .unwrap();
    }
    let prompts = capture.prompts.lock().clone();
    let wave_prompts: Vec<&String> = prompts
        .iter()
        .filter(|prompt| prompt.contains("Wave \"wave-stop\""))
        .collect();
    assert_eq!(wave_prompts.len(), 1, "{wave_prompts:?}");
    assert!(wave_prompts[0].contains("all 2 members were cancelled"));
    assert!(
        backend
            .boss
            .document()
            .waves
            .iter()
            .find(|wave| wave.id == "wave-stop")
            .unwrap()
            .resolved_at
            .is_some()
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `control`'s `setWorkspace` action is the boss-side move a client
/// drives with separate stop/switch/resume steps: the session rebinds
/// to a fresh daemon worktree, the employee record stays live, and the
/// next delivered prompt carries the move notice. A rejected switch —
/// the local target it already occupies, or a base ref Git cannot
/// resolve — leaves the employee bound to its old workspace with its
/// runtime untouched.
#[test]
fn boss_set_workspace_moves_an_employee_between_workspaces() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let root = std::env::temp_dir().join(format!("boss-set-workspace-{}", Uuid::new_v4()));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let git = |args: &[&str]| {
        let output = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(&project)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet", "-b", "main"]);
    git(&["config", "core.autocrlf", "false"]);
    std::fs::write(project.join("README.md"), "main\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Goddard Tests",
        "-c",
        "user.email=waku@example.com",
        "commit",
        "--quiet",
        "-m",
        "initial",
    ]);

    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    let persona = backend.boss.document().personas[1].id;
    let employee = backend
        .boss
        .prepare_employee(
            supervisor,
            persona,
            "Move job".into(),
            None,
            waku_protocol::boss::EmployeeGoal::Errand,
            None,
        )
        .unwrap();
    let employee_id = employee.session_id;
    backend.boss.add_employee(employee).unwrap();
    let git_project = Project::from_path(dunce::canonicalize(&project).unwrap());
    let mut child = AgentSession::new(git_project.id, ProviderKind::Codex);
    child.id = employee_id;
    child.boss_managed = true;
    {
        let mut state = backend.task_state.lock();
        state.projects.push(git_project);
        state.push_session(child);
        backend.task_store.save(&mut state).unwrap();
    }
    let mut daemon_settings = backend.settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    backend.settings.replace(daemon_settings).unwrap();
    let first_capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        employee_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(first_capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: project.clone(),
        },
    );
    let switch = |workspace: AgentWorkspace, base_branch: Option<&str>| {
        backend.handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::SetWorkspace {
                    workspace,
                    base_branch: base_branch.map(str::to_owned),
                },
            },
            &EventSink::detached(),
        )
    };

    // Rejected switches never touch the running employee.
    assert!(switch(AgentWorkspace::Local, None).is_err());
    assert!(switch(AgentWorkspace::Worktree, Some("no-such-ref")).is_err());
    assert!(switch(AgentWorkspace::Worktree, None).is_err());
    {
        let state = backend.task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == employee_id)
            .unwrap();
        assert!(session.workspace.is_local());
    }
    assert_eq!(*first_capture.shutdowns.lock(), 0);
    assert!(!backend.boss.employee(employee_id).unwrap().expired);

    // The switch itself lands; only the resume prompt's provider
    // launch fails under the test's missing binary.
    let error = switch(AgentWorkspace::Worktree, Some("main")).unwrap_err();
    assert!(
        format!("{error:#}").contains("could not be resumed"),
        "{error:#}"
    );
    let worktree_path = {
        let state = backend.task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == employee_id)
            .unwrap();
        let SessionWorkspace::Worktree {
            path, base_branch, ..
        } = &session.workspace
        else {
            panic!("expected a worktree workspace")
        };
        assert_eq!(base_branch.as_deref(), Some("main"));
        let repository = dunce::canonicalize(&project).unwrap();
        assert!(path.starts_with(repository.parent().unwrap().join("worktrees")));
        assert!(crate::worktree::is_linked_worktree(path));
        let context = session.pending_provider_context.as_ref().unwrap();
        assert!(context.contains(&path.display().to_string()));
        assert!(context.contains(&project.display().to_string()));
        path.clone()
    };
    // The old runtime was retired, the record is live again, and the
    // resume prompt waits parked for the next runtime.
    assert_eq!(*first_capture.shutdowns.lock(), 1);
    assert!(!backend.boss.employee(employee_id).unwrap().expired);
    assert!(backend.agent.has_queued(employee_id));

    // Once a runtime exists the parked resume delivers with the move
    // notice folded into it.
    let second_capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        employee_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(second_capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: worktree_path.clone(),
        },
    );
    backend
        .queue_agent_prompt_hidden(
            employee_id,
            "check in".into(),
            Some(supervisor),
            &EventSink::detached(),
        )
        .unwrap();
    {
        let prompts = second_capture.prompts.lock();
        assert_eq!(prompts.len(), 2, "{prompts:?}");
        assert!(prompts[0].contains(&worktree_path.display().to_string()));
        assert!(prompts[0].contains("Continue the job"));
    }

    // Switching back rebinds the session to the primary checkout.
    assert!(switch(AgentWorkspace::Local, None).is_err());
    {
        let state = backend.task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == employee_id)
            .unwrap();
        assert!(session.workspace.is_local());
    }
    assert_eq!(*second_capture.shutdowns.lock(), 1);
    assert!(!backend.boss.employee(employee_id).unwrap().expired);

    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `merge submit` is per-project opt-in: a worktree employee whose
/// project never enabled submissions is refused outright, an employee
/// credential cannot flip the switch itself, and the boss's
/// `setProjectSubmissions` lets the same submission land on the QA
/// branch. The flag is stored on the project row — it survives a
/// reload.
#[test]
fn merge_submit_requires_per_project_opt_in() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("waku-merge-gate-{}", Uuid::new_v4()));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let git = |cwd: &Path, args: &[&str]| {
        let output = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    git(&project, &["init", "--quiet", "-b", "main"]);
    git(&project, &["config", "core.autocrlf", "false"]);
    std::fs::write(project.join("README.md"), "main\n").unwrap();
    git(&project, &["add", "."]);
    git(
        &project,
        &[
            "-c",
            "user.name=Goddard Tests",
            "-c",
            "user.email=waku@example.com",
            "commit",
            "--quiet",
            "-m",
            "initial",
        ],
    );
    git(&project, &["branch", "dev"]);
    let dev = root.join("dev");
    git(&project, &["worktree", "add", dev.to_str().unwrap(), "dev"]);
    let employee_worktree = root.join("employee");
    git(
        &project,
        &[
            "worktree",
            "add",
            "--detach",
            employee_worktree.to_str().unwrap(),
            "main",
        ],
    );
    std::fs::write(employee_worktree.join("unit.txt"), "unit\n").unwrap();
    git(&employee_worktree, &["add", "."]);
    git(
        &employee_worktree,
        &[
            "-c",
            "user.name=Goddard Tests",
            "-c",
            "user.email=waku@example.com",
            "commit",
            "--quiet",
            "-m",
            "feat: unit",
        ],
    );

    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    let git_project = Project::from_path(dunce::canonicalize(&project).unwrap());
    assert!(!git_project.submissions_enabled);
    let employee_id = Uuid::new_v4();
    let mut child = AgentSession::new(git_project.id, ProviderKind::Codex);
    child.id = employee_id;
    child.boss_managed = true;
    child.workspace = SessionWorkspace::Worktree {
        path: employee_worktree.clone(),
        name: "employee".into(),
        branch: None,
        base_branch: Some("main".into()),
        adopted_by: None,
    };
    {
        let mut state = backend.task_state.lock();
        state.projects.push(git_project);
        state.push_session(child);
        backend.task_store.save(&mut state).unwrap();
    }

    // Off by default — the refusal fires before Git is touched.
    let error = backend.agent_merge_submit(Some(employee_id)).unwrap_err();
    assert!(
        format!("{error:#}").contains("submissions not enabled for this project"),
        "{error:#}"
    );

    // An employee credential cannot opt its own project in, and an
    // unknown reference fails instead of materializing state.
    assert!(
        backend
            .handle_boss_operation(
                Some(employee_id),
                BossOperation::SetProjectSubmissions {
                    project: "project".into(),
                    enabled: true,
                },
                &EventSink::detached(),
            )
            .is_err()
    );
    assert!(
        backend
            .handle_boss_operation(
                Some(supervisor),
                BossOperation::SetProjectSubmissions {
                    project: "no-such-project".into(),
                    enabled: true,
                },
                &EventSink::detached(),
            )
            .is_err()
    );

    // The boss opts the project in by name; the same submit lands.
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::SetProjectSubmissions {
                project: "project".into(),
                enabled: true,
            },
            &EventSink::detached(),
        )
        .unwrap();
    let ResponsePayload::AgentMergeSubmitted { sha } =
        backend.agent_merge_submit(Some(employee_id)).unwrap()
    else {
        panic!("expected a landed sha")
    };
    assert_eq!(
        git(&dev, &["rev-parse", "--verify", "HEAD"]).trim(),
        sha,
        "the QA worktree fast-forwarded to the landed unit"
    );
    assert!(dev.join("unit.txt").exists());

    // The flag lives on the project row, not in memory.
    let restored = backend.task_store.load().unwrap();
    assert!(
        restored
            .projects
            .iter()
            .find(|project| project.name == "project")
            .unwrap()
            .submissions_enabled
    );

    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A client `SaveTaskState` echoes each project as a `Project` literal —
/// daemon-owned fields arrive as defaults and must not overwrite the
/// submissions opt-in, the QA-branch override, or the legacy friend
/// marker a boss op or friend delivery wrote on the row.
#[test]
fn client_state_save_preserves_daemon_project_fields() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("waku-project-merge-{}", Uuid::new_v4()));
    let project_dir = root.join("project");
    std::fs::create_dir_all(&project_dir).unwrap();

    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    let project = Project::from_path(dunce::canonicalize(&project_dir).unwrap());
    let project_id = project.id;
    {
        let mut state = backend.task_state.lock();
        state.projects.push(project.clone());
        state
            .projects
            .iter_mut()
            .find(|row| row.id == project_id)
            .unwrap()
            .friend_peer_id = Some("peer".into());
        backend.task_store.save(&mut state).unwrap();
    }

    // The boss opts the project in and overrides its QA branch.
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::SetProjectSubmissions {
                project: "project".into(),
                enabled: true,
            },
            &EventSink::detached(),
        )
        .unwrap();
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::SetProjectQaBranch {
                project: "project".into(),
                branch: Some("release".into()),
            },
            &EventSink::detached(),
        )
        .unwrap();

    // The client's save echoes the row with its own edit — a star —
    // and defaults for every field it does not own.
    let mut echoed = project.clone();
    echoed.starred = true;
    backend
        .handle(
            waku_protocol::Request {
                request_id: Uuid::new_v4(),
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::SaveTaskState {
                    projects: vec![echoed],
                    live_session_ids: Vec::new(),
                    sessions: Vec::new(),
                    session_tails: Vec::new(),
                },
            },
            EventSink::detached(),
            None,
        )
        .unwrap();

    let state = backend.task_state.lock();
    let row = state
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .unwrap();
    assert!(
        row.submissions_enabled,
        "a client save must not revert the submissions opt-in"
    );
    assert_eq!(row.qa_branch.as_deref(), Some("release"));
    assert_eq!(row.friend_peer_id.as_deref(), Some("peer"));
    assert!(row.starred, "client-owned edits still apply");

    drop(state);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A boss `project` reference resolves through the same ownership rule
/// `project_for_path` applies — a path beneath the registered root
/// names the project — and a same-named temporary row never shadows
/// the registered one.
#[test]
fn registered_project_reference_covers_descendants_and_skips_temporaries() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("waku-project-ref-{}", Uuid::new_v4()));
    let project_dir = root.join("project");
    let nested = project_dir.join("nested");
    std::fs::create_dir_all(&nested).unwrap();

    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    // A temporary ad-hoc project shares the basename and sorts first.
    let mut temporary = Project::from_path(root.join("elsewhere"));
    temporary.name = "project".into();
    temporary.temporary = true;
    let project = Project::from_path(dunce::canonicalize(&project_dir).unwrap());
    let project_id = project.id;
    {
        let mut state = backend.task_state.lock();
        state.projects.push(temporary);
        state.projects.push(project);
        backend.task_store.save(&mut state).unwrap();
    }

    // The name resolves the registered row, not the temporary.
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::SetProjectSubmissions {
                project: "project".into(),
                enabled: true,
            },
            &EventSink::detached(),
        )
        .unwrap();
    // A path beneath the registered root names the project too.
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::SetProjectQaBranch {
                project: nested.to_string_lossy().into_owned(),
                branch: Some("release".into()),
            },
            &EventSink::detached(),
        )
        .unwrap();

    let state = backend.task_state.lock();
    let row = state
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .unwrap();
    assert!(row.submissions_enabled);
    assert_eq!(row.qa_branch.as_deref(), Some("release"));

    drop(state);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `setProjectQaBranch` retargets a project's review train: the
/// override lands `merge submit` units on it, `project_qa_branch`
/// resolves it for the project's checkout and its linked worktrees,
/// and clearing the override re-inherits the daemon-global setting.
/// An unusable branch name is rejected at the op, not at submit time.
#[test]
fn merge_submit_honors_a_per_project_qa_branch() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("waku-qa-branch-{}", Uuid::new_v4()));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let git = |cwd: &Path, args: &[&str]| {
        let output = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    git(&project, &["init", "--quiet", "-b", "main"]);
    git(&project, &["config", "core.autocrlf", "false"]);
    std::fs::write(project.join("README.md"), "main\n").unwrap();
    git(&project, &["add", "."]);
    git(
        &project,
        &[
            "-c",
            "user.name=Goddard Tests",
            "-c",
            "user.email=waku@example.com",
            "commit",
            "--quiet",
            "-m",
            "initial",
        ],
    );
    git(&project, &["branch", "dev"]);
    git(&project, &["branch", "release"]);
    let dev = root.join("dev");
    let release = root.join("release");
    git(&project, &["worktree", "add", dev.to_str().unwrap(), "dev"]);
    git(
        &project,
        &["worktree", "add", release.to_str().unwrap(), "release"],
    );
    let employee_worktree = root.join("employee");
    git(
        &project,
        &[
            "worktree",
            "add",
            "--detach",
            employee_worktree.to_str().unwrap(),
            "main",
        ],
    );
    let commit_unit = |file: &str| {
        std::fs::write(employee_worktree.join(file), "unit\n").unwrap();
        git(&employee_worktree, &["add", "."]);
        git(
            &employee_worktree,
            &[
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "--quiet",
                "-m",
                "feat: unit",
            ],
        );
    };
    commit_unit("unit-a.txt");

    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    let git_project = Project::from_path(dunce::canonicalize(&project).unwrap());
    let project_id = git_project.id;
    let employee_id = Uuid::new_v4();
    let mut child = AgentSession::new(project_id, ProviderKind::Codex);
    child.id = employee_id;
    child.boss_managed = true;
    child.workspace = SessionWorkspace::Worktree {
        path: employee_worktree.clone(),
        name: "employee".into(),
        branch: None,
        base_branch: Some("main".into()),
        adopted_by: None,
    };
    {
        let mut state = backend.task_state.lock();
        let mut git_project = git_project;
        git_project.submissions_enabled = true;
        state.projects.push(git_project);
        state.push_session(child);
        backend.task_store.save(&mut state).unwrap();
    }

    // Without an override the global setting decides: `dev`.
    let canonical_project = dunce::canonicalize(&project).unwrap();
    assert_eq!(backend.project_qa_branch(&canonical_project), "dev");
    assert_eq!(
        backend.project_qa_branch(&dunce::canonicalize(&employee_worktree).unwrap()),
        "dev",
        "a linked worktree resolves to its project's override"
    );

    // Garbage names fail the op instead of breaking a later submit.
    assert!(
        backend
            .handle_boss_operation(
                Some(supervisor),
                BossOperation::SetProjectQaBranch {
                    project: "project".into(),
                    branch: Some("-x".into()),
                },
                &EventSink::detached(),
            )
            .is_err()
    );

    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::SetProjectQaBranch {
                project: "project".into(),
                branch: Some(" release ".into()),
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert_eq!(backend.project_qa_branch(&canonical_project), "release");
    assert_eq!(
        backend.project_qa_branch(&dunce::canonicalize(&employee_worktree).unwrap()),
        "release"
    );
    // Other repositories keep the global default.
    assert_eq!(
        backend.project_qa_branch(Path::new("/tmp")),
        backend.settings.get().qa_branch
    );

    let ResponsePayload::AgentMergeSubmitted { sha } =
        backend.agent_merge_submit(Some(employee_id)).unwrap()
    else {
        panic!("expected a landed sha")
    };
    assert_eq!(
        git(&release, &["rev-parse", "--verify", "HEAD"]).trim(),
        sha,
        "the submission landed on the per-project QA branch"
    );
    assert!(release.join("unit-a.txt").exists());
    assert!(!dev.join("unit-a.txt").exists(), "dev was not advanced");

    // Clearing the override re-inherits the global setting.
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::SetProjectQaBranch {
                project: "project".into(),
                branch: None,
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert_eq!(backend.project_qa_branch(&canonical_project), "dev");
    let restored = backend.task_store.load().unwrap();
    assert!(
        restored
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .unwrap()
            .qa_branch
            .is_none(),
        "the cleared override stayed cleared across reload"
    );

    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// Scripts chain bound operations in one invocation, and non-boss
/// sessions are refused. Rhai variables do not survive the call.
#[test]
fn boss_script_batches_operations_with_fresh_scope_per_invocation() {
    use waku_protocol::boss::{BossOperation, BossResult};
    let root = std::env::temp_dir().join(format!("boss-eval-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = backend.boss.document().identity.id;
    backend.boss.set_session_id(boss).unwrap();
    let eval = |script: &str| {
        backend.handle_boss_operation(
            None,
            BossOperation::Eval {
                script: script.into(),
            },
            &EventSink::detached(),
        )
    };
    let BossResult::Eval { value, .. } = eval("let count = 40; count + 2").unwrap() else {
        panic!("expected an eval result")
    };
    assert_eq!(value, serde_json::json!(42));
    assert!(
        eval("count + 1").is_err(),
        "script variables must not persist between calls"
    );
    // Bound functions run real boss operations — a file write lands in
    // the boss's files root, and `view()` unwraps to the state map.
    let BossResult::Eval { value, .. } = eval(
        "writeFile(\"memory/eval-note.md\", \"durable fact\"); readFile(\"memory/eval-note.md\")",
    )
    .unwrap() else {
        panic!("expected an eval result")
    };
    assert_eq!(value["content"], serde_json::json!("durable fact"));
    assert_eq!(
        std::fs::read_to_string(root.join("boss/files/memory/eval-note.md")).unwrap(),
        "durable fact"
    );
    let BossResult::Eval { value, .. } = eval("view().identity.name.len() > 0").unwrap() else {
        panic!("expected an eval result")
    };
    assert_eq!(value, serde_json::json!(true));
    let BossResult::Eval { output, .. } = eval("print(\"ping\")").unwrap() else {
        panic!("expected an eval result")
    };
    assert!(output.contains("ping"));
    assert!(eval("loop { }").is_err());
    // Boss session identity does not make Rhai variables persistent.
    assert!(
        backend
            .handle_boss_operation(
                Some(boss),
                BossOperation::Eval {
                    script: "count".into(),
                },
                &EventSink::detached(),
            )
            .is_err()
    );
    backend.boss.set_session_id(Uuid::new_v4()).unwrap();
    assert!(eval("count").is_err());
    assert!(
        backend
            .handle_boss_operation(
                Some(Uuid::new_v4()),
                BossOperation::Eval { script: "1".into() },
                &EventSink::detached(),
            )
            .is_err()
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `createPlan` builds a boss-attached managed session: the registry
/// record, the planning marker, the Boss project, and the seeded
/// transcript all land before the provider launch is attempted — so a
/// missing test binary fails the call but leaves the session, like a
/// failed summon.
#[test]
fn create_plan_opens_a_seeded_managed_planning_session() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("boss-plan-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let mut daemon_settings = backend.settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    backend.settings.replace(daemon_settings).unwrap();
    let events = EventSink::detached();
    let create = |caller,
                  title: &str,
                  plan_file: &str,
                  prompt: &str,
                  provider: Option<ProviderKind>,
                  model: Option<&str>,
                  reasoning_effort: Option<&str>| {
        backend.handle_boss_operation(
            caller,
            BossOperation::CreatePlan {
                title: title.into(),
                plan_file: plan_file.into(),
                prompt: prompt.into(),
                provider,
                model: model.map(str::to_owned),
                reasoning_effort: reasoning_effort.map(str::to_owned),
            },
            &events,
        )
    };
    // Only a boss principal or a human may open a plan.
    assert!(
        create(
            Some(Uuid::new_v4()),
            "Auth",
            "auth.md",
            "plan it",
            None,
            None,
            None
        )
        .is_err()
    );
    // The launch fails on the missing binary; the session and its
    // registry record still landed.
    assert!(
        create(
            None,
            "Auth",
            "auth.md",
            "plan the auth migration",
            None,
            None,
            None
        )
        .is_err()
    );
    let plan = backend.boss.plan_for_file("plans/auth.md").unwrap();
    assert_eq!(plan.idea, "Auth");
    assert_eq!(plan.finalized_at, None);
    let session = {
        let mut state = backend.task_state.lock();
        let index = state
            .sessions
            .iter()
            .position(|session| session.id == plan.session_id)
            .unwrap();
        backend
            .task_store
            .hydrate(&mut state.sessions[index])
            .unwrap();
        state.sessions[index].clone()
    };
    assert!(session.boss_managed);
    assert!(session.is_planning());
    assert_eq!(session.title, "Auth");
    assert_eq!(session.project_id, backend.boss.document().identity.id);
    // An op that names no model lands on the planning pick — Codex's
    // sol model at medium effort — not the sending session's config.
    assert_eq!(session.provider, ProviderKind::Codex);
    assert_eq!(session.model.as_deref(), Some("gpt-6.1-sol"));
    assert_eq!(session.reasoning_effort.as_deref(), Some("medium"));
    let planning = session.planning.clone().unwrap();
    assert_eq!(planning.plan_file, "plans/auth.md");
    assert_eq!(planning.idea, "Auth");
    assert_eq!(planning.finalized_at, None);
    assert_eq!(planning.label.key, "boss.planning_label");
    let seed = &session.messages[0].content;
    assert!(seed.contains("plan the auth migration"));
    assert!(seed.contains("plans/auth.md"));
    // A second plan on the same document is refused; a different idea
    // runs alongside it, and an explicit model/effort still wins.
    assert!(
        create(
            None,
            "Auth again",
            "memory/plans/auth.md",
            "dup",
            None,
            None,
            None
        )
        .is_err()
    );
    assert!(
        create(
            None,
            "Billing",
            "billing.md",
            "plan billing",
            Some(ProviderKind::Codex),
            Some("gpt-5.5"),
            Some("high")
        )
        .is_err()
    );
    let billing_plan = backend.boss.plan_for_file("plans/billing.md").unwrap();
    {
        let mut state = backend.task_state.lock();
        let index = state
            .sessions
            .iter()
            .position(|session| session.id == billing_plan.session_id)
            .unwrap();
        backend
            .task_store
            .hydrate(&mut state.sessions[index])
            .unwrap();
        let billing = &state.sessions[index];
        assert_eq!(billing.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(billing.reasoning_effort.as_deref(), Some("high"));
    }
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A human `finalizePlan` is its own approver — no card parks — and the
/// stamp lands on the registry record, the session marker, and the
/// document freeze, while the implementation handoff parks on the boss
/// chat's durable queue. After the grace sweep the session is archived
/// like any managed task and can no longer summon.
#[test]
fn finalize_plan_freezes_the_document_then_the_grace_sweep_archives() {
    use waku_protocol::boss::{BossOperation, BossResult};
    let root = std::env::temp_dir().join(format!("boss-plan-final-{}", Uuid::new_v4()));
    // The grace sweep reaches the backend through the weak callback
    // `start_automations` installs in production — bind it directly.
    let (backend, boss) = surface_test_backend(&root);
    let backend = Arc::new(backend);
    backend.bind_boss_finish_callback();
    backend.boss.set_session_id(boss).unwrap();
    let mut daemon_settings = backend.settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    backend.settings.replace(daemon_settings).unwrap();
    let events = EventSink::detached();
    assert!(
        backend
            .handle_boss_operation(
                None,
                BossOperation::CreatePlan {
                    title: "Auth".into(),
                    plan_file: "auth.md".into(),
                    prompt: "plan the auth migration".into(),
                    provider: Some(ProviderKind::Codex),
                    model: None,
                    reasoning_effort: None,
                },
                &events,
            )
            .is_err()
    );
    let plan = backend.boss.plan_for_file("plans/auth.md").unwrap();
    let BossResult::PlanFinalized {
        session_id,
        plan_file,
        finalized_at,
    } = backend
        .handle_boss_operation(
            None,
            BossOperation::FinalizePlan {
                plan_file: Some("memory/plans/auth.md".into()),
                items: None,
            },
            &events,
        )
        .unwrap()
    else {
        panic!("expected the finalized result")
    };
    assert_eq!(session_id, plan.session_id);
    assert_eq!(plan_file, "plans/auth.md");
    assert!(finalized_at > 0);
    assert!(
        backend
            .agent
            .parked_permission_request(plan.session_id)
            .is_none(),
        "human finalization must not park another approval request"
    );
    assert_eq!(
        backend
            .boss
            .plan_for_file("plans/auth.md")
            .unwrap()
            .finalized_at,
        Some(finalized_at)
    );
    {
        let state = backend.task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == plan.session_id)
            .unwrap();
        assert_eq!(
            session.planning.as_ref().unwrap().finalized_at,
            Some(finalized_at)
        );
        assert!(session.archived_at.is_none());
    }
    // The boss chat carries the handoff: a hidden daemon-owned prompt
    // naming the frozen document, mirrored into the session document so
    // a restart cannot drop it. The boss session has no live runtime
    // here, so delivery waits in the parked queue.
    {
        let mut state = backend.task_state.lock();
        let index = state
            .sessions
            .iter()
            .position(|session| session.id == boss)
            .unwrap();
        backend
            .task_store
            .hydrate(&mut state.sessions[index])
            .unwrap();
        let handoff = state.sessions[index]
            .queued_messages
            .iter()
            .find(|queued| queued.is_agent_owned() && queued.hidden)
            .expect("the boss chat holds the parked handoff");
        assert!(handoff.content.contains("finalized its design"));
        assert!(handoff.content.contains("plans/auth.md"));
        let trigger = handoff
            .report_trigger
            .as_ref()
            .expect("the handoff records its turn trigger");
        assert_eq!(trigger.kind, crate::model::ReportTriggerKind::PlanFinalized);
        assert_eq!(trigger.employee, plan.session_id);
        assert_eq!(trigger.employee_name, "Auth");
        assert_eq!(trigger.job_title, "plans/auth.md");
    }
    // The document is frozen and re-finalizing is refused.
    assert!(
        backend
            .boss
            .handle(
                None,
                BossOperation::WriteFile {
                    path: "plans/auth.md".into(),
                    content: "edit".into(),
                },
            )
            .is_err()
    );
    assert!(
        backend
            .handle_boss_operation(
                None,
                BossOperation::FinalizePlan {
                    plan_file: Some("auth.md".into()),
                    items: None,
                },
                &events,
            )
            .is_err()
    );
    // During grace the planning session can still summon an employee;
    // its reports route back to it.
    let persona = backend.boss.document().personas[0].id;
    let employee = backend
        .boss
        .prepare_employee(
            plan.session_id,
            persona,
            "Follow-up".into(),
            None,
            waku_protocol::boss::EmployeeGoal::Errand,
            None,
        )
        .unwrap();
    assert_eq!(backend.boss.report_target(&employee), Some(plan.session_id));
    // Elapse the window and the sweep archives the session; afterwards
    // it is a plain archived managed task — no more summons, and a
    // report would escalate to the boss session instead.
    backend
        .boss
        .set_plan_finalized_at(plan.session_id, 1)
        .unwrap();
    assert_eq!(
        backend
            .boss
            .archive_graced_plans(crate::model::unix_time())
            .unwrap(),
        1
    );
    {
        let state = backend.task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == plan.session_id)
            .unwrap();
        assert!(session.archived_at.is_some());
    }
    assert!(
        backend
            .boss
            .prepare_employee(
                plan.session_id,
                persona,
                "Follow-up".into(),
                None,
                waku_protocol::boss::EmployeeGoal::Errand,
                None
            )
            .is_err()
    );
    assert_eq!(backend.boss.report_target(&employee), Some(boss));
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// A boss caller approves `finalizePlan` through a parked request card
/// — the same contract `agentProposeArchive` uses — and a denial leaves
/// the document unfrozen.
#[test]
fn a_boss_caller_finalizes_through_a_parked_request_card() {
    use waku_protocol::boss::BossOperation;
    let root = std::env::temp_dir().join(format!("boss-plan-card-{}", Uuid::new_v4()));
    let (backend, boss) = surface_test_backend(&root);
    backend.boss.set_session_id(boss).unwrap();
    let boss_capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        boss,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(boss_capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.join("repo"),
        },
    );
    let mut daemon_settings = backend.settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    backend.settings.replace(daemon_settings).unwrap();
    let events = EventSink::detached();
    for plan_file in ["auth.md", "billing.md"] {
        assert!(
            backend
                .handle_boss_operation(
                    Some(boss),
                    BossOperation::CreatePlan {
                        title: plan_file.into(),
                        plan_file: plan_file.into(),
                        prompt: format!("plan {plan_file}"),
                        provider: Some(ProviderKind::Codex),
                        model: None,
                        reasoning_effort: None,
                    },
                    &events,
                )
                .is_err()
        );
    }
    let finalize = |plan_file: &'static str| {
        let backend = &backend;
        let events = events.clone();
        move || {
            backend.handle_boss_operation(
                Some(boss),
                BossOperation::FinalizePlan {
                    plan_file: Some(plan_file.into()),
                    items: None,
                },
                &events,
            )
        }
    };
    std::thread::scope(|scope| {
        let call = scope.spawn(finalize("auth.md"));
        let request_id = loop {
            if let Some(request_id) = backend.agent.parked_permission_request(boss) {
                break request_id;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(request_id.starts_with(waku_protocol::PLAN_FINALIZE_REQUEST_PREFIX));
        backend.agent.resolve_permission(
            boss,
            &Command::Respond {
                request_id: request_id.clone(),
                option_id: "finalize".into(),
            },
        );
        assert!(call.join().unwrap().is_ok());
        assert_eq!(
            backend
                .boss
                .plan_for_file("plans/auth.md")
                .unwrap()
                .finalized_at
                .is_some(),
            true
        );
        // Approval handed implementation to the boss chat — the
        // handoff prompt drained straight into its idle runtime.
        let prompts = boss_capture.prompts.lock().clone();
        assert!(
            prompts
                .iter()
                .any(|prompt| prompt.contains("finalized its design")
                    && prompt.contains("plans/auth.md")),
            "the boss chat received the implementation handoff"
        );
        let call = scope.spawn(finalize("billing.md"));
        let request_id = loop {
            if let Some(request_id) = backend.agent.parked_permission_request(boss) {
                break request_id;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        backend.agent.resolve_permission(
            boss,
            &Command::Respond {
                request_id,
                option_id: "deny".into(),
            },
        );
        assert!(call.join().unwrap().is_err());
        assert!(
            backend
                .boss
                .plan_for_file("plans/billing.md")
                .unwrap()
                .finalized_at
                .is_none()
        );
    });
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// The planning marker is daemon-owned: a stale client save can neither
/// unfinalize a frozen plan nor drop the marker, while a marker the
/// daemon copy lacks is adopted.
#[test]
fn a_stale_client_save_cannot_unfreeze_a_planning_session() {
    use waku_protocol::model::SessionPlanning;
    let marker = |finalized_at: Option<u64>| {
        Some(SessionPlanning {
            plan_file: "plans/auth.md".into(),
            idea: "Auth".into(),
            label: waku_protocol::WireTranslation::new("boss.planning_label", []),
            finalized_at,
        })
    };
    let mut existing = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    existing.planning = marker(Some(10));
    let mut incoming = existing.clone();
    incoming.planning.as_mut().unwrap().finalized_at = None;
    incoming.updated_at = existing.updated_at + 1;
    merge_stale_session_metadata(&mut existing, incoming);
    assert_eq!(existing.planning.as_ref().unwrap().finalized_at, Some(10));

    let mut incoming = existing.clone();
    incoming.planning = None;
    incoming.updated_at = existing.updated_at + 1;
    merge_stale_session_metadata(&mut existing, incoming);
    assert!(existing.planning.is_some());

    let mut skeleton = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let mut incoming = skeleton.clone();
    incoming.planning = marker(Some(10));
    incoming.updated_at = skeleton.updated_at + 1;
    assert!(merge_session_list_columns(&mut skeleton, incoming, false));
    assert_eq!(skeleton.planning.unwrap().finalized_at, Some(10));
}

/// A live employee with no open turn gets a control prompt right away:
/// the queue's drain is the same kick a fresh prompt sends, and two
/// sends deliver in submission order.
#[test]
fn a_prompt_to_an_idle_employee_starts_a_turn() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let root = std::env::temp_dir().join(format!("boss-idle-wake-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, _parent, child) = employee_finish_fixture(&root);
    for text in ["follow up", "and another"] {
        backend
            .handle_boss_operation(
                Some(supervisor),
                BossOperation::Control {
                    session_id: employee_id,
                    action: EmployeeControl::Prompt {
                        prompt: text.into(),
                        delivery: Some(AgentPromptDelivery::Interrupt),
                    },
                },
                &EventSink::detached(),
            )
            .unwrap();
    }
    let prompts = child.prompts.lock().clone();
    assert_eq!(prompts.len(), 2);
    assert!(prompts[0].contains("follow up"));
    assert!(prompts[1].contains("and another"));
    drop(prompts);
    assert!(!backend.agent.has_queued(employee_id));
    assert!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == employee_id)
            .is_some_and(|session| session.queued_messages.is_empty())
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A supervisor's plain prompt interrupts the running turn — the
/// default delivery steers in like an explicit steer, and only an
/// explicit `queue` parks behind the open turn.
#[test]
fn a_supervisor_prompt_steers_the_running_turn() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let root = std::env::temp_dir().join(format!("boss-steer-default-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, _parent, child) = employee_finish_fixture(&root);
    backend
        .agent
        .note_driver_event(employee_id, &DriverEvent::TurnStarted);
    let prompt = |delivery| {
        backend.handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::Prompt {
                    prompt: "redirect".into(),
                    delivery: Some(delivery),
                },
            },
            &EventSink::detached(),
        )
    };
    prompt(AgentPromptDelivery::Interrupt).unwrap();
    let steers = child.steers.lock().clone();
    assert_eq!(steers.len(), 1);
    assert!(steers[0].contains("redirect"));
    drop(steers);
    assert!(child.prompts.lock().is_empty());
    assert!(!backend.agent.has_queued(employee_id));
    // An explicit queue still parks behind the open turn.
    prompt(AgentPromptDelivery::Queue).unwrap();
    assert!(backend.agent.has_queued(employee_id));
    let _ = std::fs::remove_dir_all(root);
}

/// A prompt parked behind the employee's last turn outlives the expiry
/// the finish applies: its mirrored queue entry is the revive path's
/// backlog, so the next control prompt drains it first.
#[test]
fn a_queued_prompt_survives_expiry_and_drains_before_the_revive_prompt() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let root = std::env::temp_dir().join(format!("boss-parked-finish-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, _parent, child) = employee_finish_fixture(&root);
    backend
        .agent
        .note_driver_event(employee_id, &DriverEvent::TurnStarted);
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::Prompt {
                    prompt: "queued while working".into(),
                    delivery: Some(AgentPromptDelivery::Queue),
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert!(child.prompts.lock().is_empty());
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    assert!(backend.boss.employee(employee_id).unwrap().expired);
    {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == employee_id)
            .unwrap();
        backend.task_store.hydrate(session).unwrap();
        assert_eq!(session.queued_messages.len(), 1);
        assert_eq!(session.queued_messages[0].content, "queued while working");
    }
    // The expiry pass dropped the runtime; a revived prompt would
    // cold-start one in production — a control driver stands in here.
    backend.sessions.lock().insert(
        employee_id,
        RuntimeEntry {
            runtime_id: Uuid::new_v4(),
            driver: DriverHandle::from_control(child.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::Prompt {
                    prompt: "revive prompt".into(),
                    delivery: Some(AgentPromptDelivery::Interrupt),
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    let prompts = child.prompts.lock().clone();
    assert_eq!(prompts.len(), 2);
    assert!(prompts[0].contains("queued while working"));
    assert!(prompts[1].contains("revive prompt"));
    let _ = std::fs::remove_dir_all(root);
}

/// The revive drain publishes on the session's runtime stream: a client
/// watching the employee's page adopts `promptSubmitted` into the resumed
/// turn and drops the parked chip on `queuedMessagesChanged`. Emitted on
/// the root event source instead, both target `(nil, nil)` and the hub
/// drops them — the stored turn exists but no attached or replaying
/// client ever learns it opened.
#[test]
fn a_revived_employee_prompt_reaches_session_subscribers() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let root = std::env::temp_dir().join(format!("boss-revive-events-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, _parent, child) = employee_finish_fixture(&root);
    backend
        .finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    // The expiry pass dropped the runtime; a revived prompt would
    // cold-start one in production — a control driver stands in here.
    let employee_runtime = Uuid::new_v4();
    backend.sessions.lock().insert(
        employee_id,
        RuntimeEntry {
            runtime_id: employee_runtime,
            driver: DriverHandle::from_control(child.clone()),
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );
    // `serve` installs the hub's root sink as the event source — scoped
    // to no session — so the stand-in runtime is registered on the hub
    // separately and the tap plays an attached client.
    let source = EventSink::detached();
    let _registered = source.begin_session_runtime(employee_id, employee_runtime);
    *backend.event_source.lock() = source.clone();
    let tapped = source.tapped_events();
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::Prompt {
                    prompt: "revive prompt".into(),
                    delivery: Some(AgentPromptDelivery::Interrupt),
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    let kinds = std::iter::from_fn(|| tapped.try_recv().ok())
        .filter_map(|message| match message {
            crate::ServerMessage::Event(event) => Some(event),
            _ => None,
        })
        .filter(|event| event.session_id == employee_id)
        .map(|event| (event.runtime_id, event.event.kind))
        .collect::<Vec<_>>();
    assert!(
        kinds
            .iter()
            .any(|(runtime_id, kind)| *runtime_id == employee_runtime && kind == "promptSubmitted"),
        "expected promptSubmitted on the employee's stream, got {kinds:?}"
    );
    assert!(
        kinds
            .iter()
            .any(|(runtime_id, kind)| *runtime_id == employee_runtime
                && kind == "queuedMessagesChanged"),
        "expected queuedMessagesChanged on the employee's stream, got {kinds:?}"
    );
    let _ = std::fs::remove_dir_all(root);
}
/// Build a summon op over the shared fixture — `plan`/`item` vary,
/// everything else stays on the valid baseline the other summon
/// tests use.
fn summon_with_tag(
    persona: Uuid,
    project: &Path,
    plan: Option<String>,
    item: Option<Uuid>,
    request_id: Option<Uuid>,
) -> waku_protocol::boss::BossOperation {
    waku_protocol::boss::BossOperation::Summon {
        persona_id: persona,
        job_title: "Tagged job".into(),
        prompt: "Summarize the diff".into(),
        project: project.display().to_string(),
        provider: Some(ProviderKind::Codex),
        model: None,
        reasoning_effort: None,
        workspace: None,
        base_branch: None,
        adopt_worktree: None,
        permissions: None,
        work_goal: waku_protocol::boss::EmployeeGoal::Errand,
        icon: None,
        resources: None,
        allow_burst: false,
        group_id: None,
        priority: None,
        goal_id: None,
        plan,
        item,
        request_id,
    }
}

/// A plan record pushed straight into the Boss document — the
/// daemon path tests drive the service, not `createPlan`.
fn boss_plan(plan_file: &str, items: &[&str]) -> waku_protocol::boss::BossPlan {
    use waku_protocol::boss::{PlanItem, PlanItemState};
    waku_protocol::boss::BossPlan {
        id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        plan_file: plan_file.into(),
        idea: "Plan".into(),
        finalized_at: None,
        items: items
            .iter()
            .map(|title| PlanItem {
                id: Uuid::new_v4(),
                title: (*title).into(),
                state: PlanItemState::ToDo,
                history: Vec::new(),
            })
            .collect(),
        outcome: None,
        history: Vec::new(),
    }
}

/// Summon tagging resolves the plan and item at admission: the
/// stored link is the pair the group view consumes. The missing
/// provider binary fails the launch after the roster record lands,
/// the same path the worktree summon test relies on.
#[test]
fn boss_summon_plan_tag_persists_and_validates_references() {
    use waku_protocol::boss::PlanOutcome;
    let root = std::env::temp_dir().join(format!("boss-summon-plan-{}", Uuid::new_v4()));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let (backend, boss) = surface_test_backend(&root);
    backend.boss.set_session_id(boss).unwrap();
    let mut daemon_settings = backend.settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    backend.settings.replace(daemon_settings).unwrap();
    let persona = backend.boss.document().personas[1].id;
    let events = EventSink::detached();

    // A still-open draft plan tags; the item link rides along.
    let plan = boss_plan("plans/auth.md", &["Probe", "Verify"]);
    let plan_id = plan.id;
    let plan_session = plan.session_id;
    let item = plan.items[0].id;
    backend.boss.add_plan(plan).unwrap();
    let result = backend.handle_boss_operation(
        Some(boss),
        summon_with_tag(persona, &project, Some("auth.md".into()), Some(item), None),
        &events,
    );
    assert!(result.is_err(), "the launch fails; admission tagged first");
    let employee = &backend.boss.document().employees[0];
    assert_eq!(employee.plan_id, Some(plan_id));
    assert_eq!(employee.item_id, Some(item));

    // Unknown plans, orphan items, and unknown items fail admission.
    let count = || backend.boss.document().employees.len();
    for (plan, item, want) in [
        (Some("plans/missing.md".into()), None, "unknown plan"),
        (None, Some(item), "requires a plan"),
        (
            Some("auth.md".into()),
            Some(Uuid::new_v4()),
            "unknown work item",
        ),
    ] {
        let result = backend.handle_boss_operation(
            Some(boss),
            summon_with_tag(persona, &project, plan, item, None),
            &events,
        );
        let message = result.unwrap_err().to_string();
        assert!(message.contains(want), "{message} should mention {want}");
        assert_eq!(count(), 1, "{want} added no employee");
    }

    // Marking the item done rejects new tags for it; the plan still
    // tags unallocated.
    backend
        .boss
        .set_plan_item_state(
            Some(boss),
            "plans/auth.md",
            item,
            waku_protocol::boss::PlanItemState::Done,
        )
        .unwrap();
    let result = backend.handle_boss_operation(
        Some(boss),
        summon_with_tag(persona, &project, Some("auth.md".into()), Some(item), None),
        &events,
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("reopen it to tag work")
    );
    let result = backend.handle_boss_operation(
        Some(boss),
        summon_with_tag(persona, &project, Some("auth.md".into()), None, None),
        &events,
    );
    assert!(result.is_err(), "the launch fails; the tag still landed");
    let employee = &backend.boss.document().employees[1];
    assert_eq!(employee.plan_id, Some(plan_id));
    assert_eq!(employee.item_id, None);

    // A closed plan refuses every tag until reopened. Approving the
    // draft first moves it into the outcome lifecycle.
    backend.boss.set_plan_finalized_at(plan_session, 1).unwrap();
    backend
        .boss
        .set_plan_outcome(Some(boss), "plans/auth.md", PlanOutcome::Abandoned)
        .unwrap();
    let result = backend.handle_boss_operation(
        Some(boss),
        summon_with_tag(persona, &project, Some("auth.md".into()), None, None),
        &events,
    );
    assert!(result.unwrap_err().to_string().contains("abandoned"));
    assert_eq!(count(), 2);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `requestId` reuse with a different plan tag is a different summon —
/// the fingerprint covers the tag.
#[test]
fn boss_summon_request_id_fingerprint_covers_the_plan_tag() {
    let root = std::env::temp_dir().join(format!("boss-summon-fp-{}", Uuid::new_v4()));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let (backend, boss) = surface_test_backend(&root);
    backend.boss.set_session_id(boss).unwrap();
    let mut daemon_settings = backend.settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    backend.settings.replace(daemon_settings).unwrap();
    let persona = backend.boss.document().personas[1].id;
    let events = EventSink::detached();
    backend
        .boss
        .add_plan(boss_plan("plans/auth.md", &[]))
        .unwrap();
    backend
        .boss
        .add_plan(boss_plan("plans/billing.md", &[]))
        .unwrap();

    let request_id = Uuid::new_v4();
    let first = backend.handle_boss_operation(
        Some(boss),
        summon_with_tag(
            persona,
            &project,
            Some("auth.md".into()),
            None,
            Some(request_id),
        ),
        &events,
    );
    assert!(first.is_err(), "the launch fails; the record stands");
    // The identical retry returns the original, tag and all.
    let retry = backend.handle_boss_operation(
        Some(boss),
        summon_with_tag(
            persona,
            &project,
            Some("auth.md".into()),
            None,
            Some(request_id),
        ),
        &events,
    );
    assert!(matches!(
        retry.unwrap(),
        waku_protocol::boss::BossResult::Summoned { .. }
    ));
    assert_eq!(backend.boss.document().employees.len(), 1);
    // Same request id, different plan — a different summon.
    let conflict = backend.handle_boss_operation(
        Some(boss),
        summon_with_tag(
            persona,
            &project,
            Some("billing.md".into()),
            None,
            Some(request_id),
        ),
        &events,
    );
    assert!(conflict.unwrap_err().to_string().contains("already used"));
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

/// `control`'s `setPlan` re-tags an admitted employee through the
/// same validation a summon tag gets.
#[test]
fn boss_control_set_plan_retags_an_admitted_employee() {
    use waku_protocol::boss::{BossOperation, EmployeeControl};
    let root = std::env::temp_dir().join(format!("boss-retag-{}", Uuid::new_v4()));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let (backend, boss) = surface_test_backend(&root);
    backend.boss.set_session_id(boss).unwrap();
    let mut daemon_settings = backend.settings.get();
    daemon_settings.provider_binary_overrides.insert(
        ProviderKind::Codex,
        root.join("missing-codex").display().to_string(),
    );
    backend.settings.replace(daemon_settings).unwrap();
    let persona = backend.boss.document().personas[1].id;
    let events = EventSink::detached();
    let plan = boss_plan("plans/auth.md", &["Probe"]);
    let item = plan.items[0].id;
    backend.boss.add_plan(plan).unwrap();
    backend
        .boss
        .add_plan(boss_plan("plans/billing.md", &[]))
        .unwrap();

    let _ = backend.handle_boss_operation(
        Some(boss),
        summon_with_tag(persona, &project, None, None, None),
        &events,
    );
    let employee = backend.boss.document().employees[0].session_id;
    let control = |action: EmployeeControl| {
        backend.handle_boss_operation(
            Some(boss),
            BossOperation::Control {
                session_id: employee,
                action,
            },
            &events,
        )
    };
    // Tag to plan + item, then re-tag to another plan — the item
    // link drops because it wasn't restated.
    control(EmployeeControl::SetPlan {
        plan: Some(Some("auth.md".into())),
        item: Some(Some(item)),
    })
    .unwrap();
    let record = &backend.boss.document().employees[0];
    assert_eq!(record.item_id, Some(item));
    control(EmployeeControl::SetPlan {
        plan: Some(Some("billing.md".into())),
        item: None,
    })
    .unwrap();
    let record = &backend.boss.document().employees[0];
    assert_eq!(
        record.plan_id,
        backend
            .boss
            .plan_for_file("plans/billing.md")
            .map(|plan| plan.id)
    );
    assert_eq!(record.item_id, None);
    // An item on another plan does not relink through this plan.
    assert!(
        control(EmployeeControl::SetPlan {
            plan: None,
            item: Some(Some(item)),
        })
        .is_err()
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

fn rotation_boss(backend: &WakuBackend) -> Uuid {
    use waku_protocol::boss::{BossOperation, BossResult};
    let BossResult::Session { session, .. } = backend
        .handle_boss_operation(
            None,
            BossOperation::Open {
                provider: ProviderKind::Codex,
                model: Some("gpt-6.1-sol".into()),
                mode: Default::default(),
            },
            &EventSink::detached(),
        )
        .unwrap()
    else {
        panic!("expected boss chat")
    };
    session.id
}

#[test]
fn boss_rotation_swaps_archives_and_continues_with_a_durable_handoff() {
    use crate::boss_rotation::RotationJournal;
    let root = std::env::temp_dir().join(format!("boss-rotation-runtime-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    let capture = Arc::new(CaptureDriver::default());
    let driver = DriverHandle::from_control(capture.clone());
    let old_runtime = Uuid::new_v4();
    backend.sessions.lock().insert(
        old,
        RuntimeEntry {
            runtime_id: old_runtime,
            driver: driver.clone(),
            last_active: std::time::Instant::now(),
            resumable: true,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(old).unwrap();
        session.reasoning_effort = Some("high".into());
        session.begin_turn("Keep supervising the release");
        session.push_message(
            MessageRole::Assistant,
            "The release work is still in flight",
        );
        backend.task_store.save(&mut state).unwrap();
    }
    // Exercise the daemon's actual recorder, including partial provider
    // usage reports, rather than seeding a client-owned usage snapshot.
    record_boss_event(
        &backend.task_state,
        &backend.task_store,
        old,
        &DriverEvent::UsageUpdated {
            context_tokens: Some(81),
            context_window: None,
        },
    )
    .unwrap();
    record_boss_event(
        &backend.task_state,
        &backend.task_store,
        old,
        &DriverEvent::UsageUpdated {
            context_tokens: None,
            context_window: Some(100),
        },
    )
    .unwrap();
    backend
        .reconcile_boss_rotation(crate::model::unix_time() + 301)
        .unwrap();
    assert_eq!(
        backend.boss.identity_and_session().1,
        Some(old),
        "open turns never rotate"
    );
    record_boss_event(
        &backend.task_state,
        &backend.task_store,
        old,
        &DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None,
        },
    )
    .unwrap();
    let refreshed = backend
        .task_state
        .lock()
        .session_mut(old)
        .unwrap()
        .last_reply_at
        .unwrap();
    backend.reconcile_boss_rotation(refreshed + 299).unwrap();
    assert_eq!(
        backend.boss.identity_and_session().1,
        Some(old),
        "warm caches never rotate"
    );
    backend.reconcile_boss_rotation(refreshed + 300).unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    assert_ne!(old, next);
    assert_eq!(*capture.shutdowns.lock(), 1);
    assert!(!backend.sessions.lock().contains_key(&old));
    assert!(
        backend
            .ensure_agent_runtime(old, &EventSink::detached())
            .err()
            .expect("archived chat must not restart")
            .to_string()
            .contains("archived")
    );
    assert_eq!(backend.boss.report_target_for(old), Some(next));
    let journal_path = root.join("boss/rotation.json");
    let journal = RotationJournal::load(&journal_path).unwrap();
    assert_eq!(journal.active_session_id, Some(next));
    assert_eq!(journal.generation, 1);
    assert!(journal.intent.is_none());
    assert_eq!(journal.rotations.len(), 1);
    assert_eq!(journal.rotations[0].old_session_id, old);
    assert_eq!(journal.rotations[0].new_session_id, next);
    assert!(journal.rotations[0].committed_at.is_some());
    // Read both transcripts back from the real database, proving archive
    // preserves the source and staging survives restart.
    let mut stored = backend.task_store.load().unwrap();
    for id in [old, next] {
        let session = stored.session_mut(id).unwrap();
        backend.task_store.hydrate(session).unwrap();
        let marker = session
            .messages
            .iter()
            .find(|message| message.content.starts_with("Boss session rotated:"))
            .unwrap();
        assert_eq!(marker.role, MessageRole::System);
        assert!(
            marker
                .content
                .contains("boss_rotation_context_threshold=0.8")
        );
        assert!(marker.content.contains("context_tokens=81"));
        assert!(marker.content.contains(&old.to_string()));
        assert!(marker.content.contains(&next.to_string()));
    }
    let transcript = backend
        .handle_boss_operation(
            Some(next),
            waku_protocol::boss::BossOperation::Transcript {
                session_id: old,
                turn: None,
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert!(
        serde_json::to_string(&transcript)
            .unwrap()
            .contains("Keep supervising the release"),
        "the new Boss can retrieve the archived conversation through its transcript API"
    );
    let previous = stored.session_mut(old).unwrap();
    assert!(previous.archived_at.is_some());
    assert!(
        previous
            .messages
            .iter()
            .any(|message| message.content == "Keep supervising the release")
    );
    let fresh = stored.session_mut(next).unwrap();
    assert!(fresh.archived_at.is_none());
    assert!(fresh.boss_managed);
    assert_eq!(fresh.model.as_deref(), Some("gpt-6.1-sol"));
    assert_eq!(fresh.reasoning_effort.as_deref(), Some("high"));
    assert!(fresh.provider_cursor.is_none());
    assert!(fresh.context_usage.is_none());
    let next_runtime = Uuid::new_v4();
    backend.sessions.lock().insert(
        next,
        RuntimeEntry {
            runtime_id: next_runtime,
            driver,
            last_active: std::time::Instant::now(),
            resumable: false,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );
    let prompt = |id, runtime_id| {
        backend.handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: id,
                runtime_id,
                command: Command::Prompt {
                    prompt: "Continue the release".into(),
                    turn_id: None,
                    message_id: None,
                    hidden: false,
                    attachments: Vec::new(),
                },
            },
            EventSink::detached(),
            None,
        )
    };
    assert!(
        prompt(old, old_runtime)
            .unwrap_err()
            .to_string()
            .contains("archived")
    );
    prompt(next, next_runtime).unwrap();
    let prompts = capture.prompts.lock();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("Continue the release"));
    assert!(prompts[0].contains("Keep supervising the release"));
    assert!(prompts[0].contains(&format!("boss transcript {old}")));
    drop(prompts);
    assert!(
        backend
            .task_state
            .lock()
            .session_mut(next)
            .unwrap()
            .pending_provider_context
            .is_none()
    );
    let recovered_boss = crate::boss::BossService::open(root.join("boss")).unwrap();
    assert_eq!(recovered_boss.identity_and_session().1, Some(next));
    backend.reconcile_boss_rotation(refreshed + 600).unwrap();
    assert_eq!(
        RotationJournal::load(&journal_path).unwrap().generation,
        1,
        "new chat does not inherit context pressure"
    );
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn boss_rotation_opt_out_and_exact_threshold_keep_the_current_chat() {
    let root = std::env::temp_dir().join(format!("boss-rotation-opt-out-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    let mut settings = backend.settings.get();
    assert!(!settings.boss_rotation_disabled);
    assert!(
        !serde_json::from_value::<crate::DaemonSettings>(json!({}))
            .unwrap()
            .boss_rotation_disabled
    );
    settings
        .boss_rotation_cache_ttl_secs
        .insert(ProviderKind::Codex, 0);
    backend.settings.replace(settings.clone()).unwrap();
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(old).unwrap();
        session.context_usage = Some(crate::model::ContextUsage {
            tokens: 80,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    settings.boss_rotation_disabled = true;
    backend.settings.replace(settings.clone()).unwrap();
    backend
        .task_state
        .lock()
        .session_mut(old)
        .unwrap()
        .context_usage
        .as_mut()
        .unwrap()
        .tokens = 81;
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    settings.boss_rotation_disabled = false;
    backend.settings.replace(settings).unwrap();
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    assert_ne!(backend.boss.identity_and_session().1, Some(old));
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn boss_rotation_recovers_each_interrupted_publication_boundary_once() {
    use crate::boss_rotation::RotationJournal;
    for boundary in 0..3 {
        let root = std::env::temp_dir().join(format!("boss-rotation-recovery-{}", Uuid::new_v4()));
        let (backend, _) = surface_test_backend(&root);
        let old = rotation_boss(&backend);
        {
            let mut state = backend.task_state.lock();
            let session = state.session_mut(old).unwrap();
            session.begin_turn("Keep supervising the existing work");
            session.finish_active_turn(TurnStatus::Completed);
            backend.task_store.save(&mut state).unwrap();
        }
        let next = Uuid::new_v4();
        let identity = backend.boss.identity_and_session().0;
        let mut journal = RotationJournal {
            active_session_id: Some(old),
            ..Default::default()
        };
        let now = crate::model::unix_time();
        journal.begin(identity.id, old, next, now).unwrap();
        journal.intent.as_mut().unwrap().reason = Some("boss_rotation_context_threshold=0.8 exceeded (context_tokens=81, context_window=100); provider prompt cache cold".into());
        journal.persist(&root.join("boss/rotation.json")).unwrap();
        if boundary >= 1 {
            let mut state = backend.task_state.lock();
            let mut staged = AgentSession::new(identity.id, ProviderKind::Codex);
            staged.id = next;
            staged.boss_managed = true;
            staged.title = identity.name;
            // Staging initializes a durable transcript and handoff; bare
            // drafts are deliberately excluded by StateStore::save.
            staged.push_message(MessageRole::System, "Boss rotation handoff staged");
            staged.pending_provider_context = Some(format!("Continue supervising work from {old}"));
            state.push_session(staged);
            backend.task_store.save(&mut state).unwrap();
        }
        if boundary == 2 {
            backend.boss.replace_session_id(old, next).unwrap();
        }
        // An opt-out must not strand a partially published rotation.
        let mut settings = backend.settings.get();
        settings.boss_rotation_disabled = true;
        backend.settings.replace(settings).unwrap();
        drop(backend);
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        backend.reconcile_boss_rotation(now + 1).unwrap();
        backend.reconcile_boss_rotation(now + 2).unwrap();
        assert_eq!(backend.boss.identity_and_session().1, Some(next));
        let journal = RotationJournal::load(&root.join("boss/rotation.json")).unwrap();
        assert_eq!(journal.generation, 1);
        assert_eq!(journal.rotations.len(), 1);
        let mut state = backend.task_store.load().unwrap();
        let old_session = state.session_mut(old).unwrap();
        backend.task_store.hydrate(old_session).unwrap();
        assert!(old_session.archived_at.is_some());
        assert_eq!(
            old_session
                .messages
                .iter()
                .filter(|message| message.content.starts_with("Boss session rotated:"))
                .count(),
            1
        );
        assert_eq!(
            state
                .sessions
                .iter()
                .filter(|session| session.id == next)
                .count(),
            1
        );
        drop(backend);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn employee_tool_outputs_survive_48_hours_after_expiry_and_retirement() {
    let root = std::env::temp_dir().join(format!("employee-retention-{}", Uuid::new_v4()));
    let (backend, supervisor) = surface_test_backend(&root);
    backend.boss.set_session_id(supervisor).unwrap();
    let persona = backend.boss.document().personas[1].id;
    let employee = backend
        .boss
        .prepare_employee(
            supervisor,
            persona,
            "Evidence".into(),
            None,
            waku_protocol::boss::EmployeeGoal::Errand,
            None,
        )
        .unwrap();
    let id = employee.session_id;
    backend.boss.add_employee(employee).unwrap();
    let now = crate::model::unix_time();
    {
        let mut state = backend.task_state.lock();
        let mut child = AgentSession::new(state.sessions[0].project_id, ProviderKind::Codex);
        child.id = id;
        let turn = child.begin_turn("Run checks and read a file");
        child.finish_active_turn(crate::model::TurnStatus::Completed);
        child.archived_at = Some(now - ARCHIVED_SESSION_RETENTION_SECONDS - 1);
        child.transcript_blocks.push(TranscriptBlock {
            after_message: 1,
            turn_id: Some(turn),
            activities: vec![
                ActivityItem::new(
                    None,
                    crate::model::ActivityKind::Command,
                    "exec",
                    None,
                    true,
                )
                .with_arguments(Some("x".repeat(10_000)))
                .with_output(Some("checks passed: command evidence".into())),
                ActivityItem::new(
                    None,
                    crate::model::ActivityKind::Tool,
                    "Read file",
                    None,
                    true,
                )
                .with_output(Some("file evidence".into())),
            ],
        });
        state.push_session(child);
        backend.task_store.save(&mut state).unwrap();
    }
    backend
        .boss
        .set_employee_expired_at(id, now - 24 * 60 * 60)
        .unwrap();
    assert_eq!(backend.boss.retire_expired(now).unwrap().len(), 1);
    // Even an archive already older than the outer purge survives next day.
    backend.purge_expired_archived_sessions();
    assert!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .any(|session| session.id == id)
    );
    let document = backend.boss.document();
    for age in [24 * 60 * 60, EMPLOYEE_TRANSCRIPT_RETENTION_SECONDS] {
        let expired_at = document
            .retired_employees
            .iter()
            .find(|employee| employee.session_id == id)
            .unwrap()
            .expired_at
            .unwrap();
        let protected = protected_employee_transcripts(&document, expired_at + age);
        assert!(protected.contains(&id));
        assert_eq!(
            backend
                .task_store
                .prune_archived_session_details(now, 8, document.identity.id, &protected)
                .unwrap(),
            0
        );
        // Reopen storage, not a resident copy, and read through the boss's
        // actual transcript projection. Large exec arguments cannot hide output.
        let reopened = StateStore::daemon(root.join("app.db"));
        let mut restored = reopened.load().unwrap();
        let session = restored
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .unwrap();
        reopened.hydrate(session).unwrap();
        for turn in [None, Some(1)] {
            let transcript = session.agent_transcript(turn);
            assert!(
                transcript
                    .items
                    .iter()
                    .any(|item| item.content.contains("command evidence"))
            );
            assert!(
                transcript
                    .items
                    .iter()
                    .any(|item| item.content.contains("file evidence"))
            );
        }
    }
    let expired_at = document
        .retired_employees
        .iter()
        .find(|employee| employee.session_id == id)
        .unwrap()
        .expired_at
        .unwrap();
    let protected = protected_employee_transcripts(
        &document,
        expired_at + EMPLOYEE_TRANSCRIPT_RETENTION_SECONDS + 1,
    );
    assert!(!protected.contains(&id));
    assert_eq!(
        backend
            .task_store
            .prune_archived_session_details(now, 8, document.identity.id, &protected)
            .unwrap(),
        1
    );
    std::fs::remove_dir_all(root).ok();
}
