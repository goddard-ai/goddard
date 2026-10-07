//! Server integration checks requiring the daemon backend.

use crate::daemon::WakuBackend;
use crate::model::{AgentSession, Project, ProviderKind, SessionStatus};
use crate::persistence::StateStore;
use crate::server::ServerOptions;
use crate::settings::DaemonSettingsStore;
use crate::{
    Command, DaemonSettings, ResponsePayload, SequencedEvent, WireDriverStartOptions, serve,
};
use base64::Engine as _;
use crossbeam_channel::{Receiver, bounded};
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use uuid::Uuid;
use waku_client::DaemonClient;

#[cfg(unix)]
#[test]
fn stale_projection_cannot_resurrect_a_removed_session() {
    let root = std::env::temp_dir().join(format!("waku-remove-race-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            Arc::new(backend),
            server_shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..ServerOptions::default()
            },
        )
        .unwrap()
    });

    let stale_client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
    let remover = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
    let project = Project::from_path(root.join("repo"));
    let mut session = AgentSession::new(project.id, ProviderKind::Codex);
    session.begin_turn("persist me");
    stale_client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: vec![project.clone()],
                live_session_ids: vec![session.id],
                sessions: vec![session.clone()],
                session_tails: Vec::new(),
            },
        )
        .unwrap();
    remover
        .request(session.id, Uuid::nil(), Command::RemoveSession)
        .unwrap();
    let ResponsePayload::TaskStateSaved { sessions } = stale_client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: vec![project],
                live_session_ids: vec![session.id],
                sessions: vec![session],
                session_tails: Vec::new(),
            },
        )
        .unwrap()
    else {
        panic!("expected task-state save response");
    };
    assert!(sessions.is_empty());
    let ResponsePayload::TaskState { sessions, .. } = stale_client
        .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
        .unwrap()
    else {
        panic!("expected task state");
    };
    assert!(sessions.is_empty());

    stale_client.shutdown();
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn an_unstarted_draft_is_never_catalogued() {
    let root = std::env::temp_dir().join(format!("waku-draft-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            Arc::new(backend),
            server_shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..ServerOptions::default()
            },
        )
        .unwrap()
    });

    let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
    let project = Project::from_path(root.join("repo"));
    // A detail-loaded draft: real to the client, but it owns no row.
    let draft = AgentSession::new(project.id, ProviderKind::Codex);
    let ResponsePayload::TaskStateSaved { sessions } = client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: vec![project.clone()],
                live_session_ids: vec![draft.id],
                sessions: vec![draft.clone()],
                session_tails: Vec::new(),
            },
        )
        .unwrap()
    else {
        panic!("expected task-state save response");
    };
    assert!(sessions.is_empty());
    let ResponsePayload::TaskState { sessions, .. } = client
        .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
        .unwrap()
    else {
        panic!("expected task state");
    };
    assert!(sessions.is_empty());

    // Once the draft starts it is catalogued like any other session.
    let mut started = draft;
    started.begin_turn("run it");
    let ResponsePayload::TaskStateSaved { sessions } = client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: vec![project],
                live_session_ids: vec![started.id],
                sessions: vec![started.clone()],
                session_tails: Vec::new(),
            },
        )
        .unwrap()
    else {
        panic!("expected task-state save response");
    };
    assert_eq!(sessions.len(), 1);
    let ResponsePayload::TaskState { sessions, .. } = client
        .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
        .unwrap()
    else {
        panic!("expected task state");
    };
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, started.id);

    client.shutdown();
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// The projectless workspace root is a process-global slot, so tests that
/// need it point it at one shared throwaway directory — every test writes
/// the same value, making the set idempotent regardless of ordering, and
/// per-test paths stay unique through their uuid names.
#[cfg(unix)]
fn projectless_test_root() -> PathBuf {
    std::env::temp_dir()
        .join("waku-projectless-test-root")
        .join("projects")
}

/// Clients that provision a projectless workspace persist its project
/// row before the first prompt's session exists; the save's orphan sweep
/// must leave a freshly created row alone or the submit fails on a
/// project the daemon forgot it just catalogued. Classification is
/// path-based, so the workspace directory never needs to exist.
#[cfg(unix)]
#[test]
fn a_fresh_projectless_project_survives_until_its_first_task() {
    crate::projectless::set_workspace_root(Some(projectless_test_root()));
    let root = std::env::temp_dir().join(format!("waku-projectless-gc-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            Arc::new(backend),
            server_shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..ServerOptions::default()
            },
        )
        .unwrap()
    });

    let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
    let workspace_root = projectless_test_root();
    let project = Project::from_path(
        workspace_root
            .join("2026-01-01")
            .join(format!("grace-{}", Uuid::new_v4())),
    );
    assert!(project.is_projectless());
    client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: vec![project.clone()],
                live_session_ids: vec![],
                sessions: vec![],
                session_tails: Vec::new(),
            },
        )
        .unwrap();
    let ResponsePayload::TaskState { projects, .. } = client
        .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
        .unwrap()
    else {
        panic!("expected task state");
    };
    assert!(
        projects.iter().any(|item| item.id == project.id),
        "a pending projectless project must outlive the save"
    );

    // A row past the grace window with no task is still swept — the
    // window only covers provisioning, not abandoned orphans.
    let mut stale = Project::from_path(
        workspace_root
            .join("2026-01-01")
            .join(format!("orphan-{}", Uuid::new_v4())),
    );
    stale.created_at = crate::model::unix_time() - 2 * 24 * 60 * 60;
    client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: vec![stale.clone()],
                live_session_ids: vec![],
                sessions: vec![],
                session_tails: Vec::new(),
            },
        )
        .unwrap();
    let ResponsePayload::TaskState { projects, .. } = client
        .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
        .unwrap()
    else {
        panic!("expected task state");
    };
    assert!(
        !projects.iter().any(|item| item.id == stale.id),
        "an orphaned projectless project must still be swept"
    );

    // A task-claimed row survives regardless of age.
    let mut claimed = Project::from_path(
        workspace_root
            .join("2026-01-01")
            .join(format!("claimed-{}", Uuid::new_v4())),
    );
    claimed.created_at = stale.created_at;
    let mut session = AgentSession::new(claimed.id, ProviderKind::Codex);
    session.begin_turn("run it");
    client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: vec![claimed.clone()],
                live_session_ids: vec![session.id],
                sessions: vec![session],
                session_tails: Vec::new(),
            },
        )
        .unwrap();
    let ResponsePayload::TaskState { projects, .. } = client
        .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
        .unwrap()
    else {
        panic!("expected task state");
    };
    assert!(
        projects.iter().any(|item| item.id == claimed.id),
        "a projectless project with a task is never swept"
    );

    client.shutdown();
    server.join().unwrap();
    std::fs::remove_dir_all(root).ok();
}

/// A projectless workspace directory can vanish between draft creation
/// and the first prompt — trash emptied, archive cleanup, a stale
/// listing. The daemon owns the scratch space, so Start recreates it
/// instead of dying inside the provider spawn with an opaque ENOENT.
#[cfg(unix)]
#[test]
fn start_recreates_a_missing_projectless_workspace() {
    let root = std::env::temp_dir().join(format!("waku-start-cwd-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    // The daemon's restore writes land under the shared test root, not
    // the real home.
    crate::projectless::set_workspace_root(Some(projectless_test_root()));
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            Arc::new(backend),
            server_shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..ServerOptions::default()
            },
        )
        .unwrap()
    });

    let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
    let missing_workspace = projectless_test_root()
        .join("2026-01-01")
        .join(format!("gone-{}", Uuid::new_v4()));
    let mut options = WireDriverStartOptions {
        binary: PathBuf::from("/nonexistent/waku-test-provider"),
        cwd: missing_workspace.clone(),
        ..test_start_options()
    };
    // The launch still fails — no provider binary — but the missing
    // projectless cwd is recreated first.
    let _ = client.request(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Command::Start {
            options: options.clone(),
        },
    );
    assert!(
        missing_workspace.is_dir(),
        "start must recreate a missing projectless workspace"
    );

    // An ordinary missing cwd names the path instead of spawning blind.
    options.cwd = root.join("repo-that-was-deleted");
    let error = client
        .request(Uuid::new_v4(), Uuid::new_v4(), Command::Start { options })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("the task's working directory does not exist"),
        "{error}"
    );
    assert!(!root.join("repo-that-was-deleted").exists());

    client.shutdown();
    server.join().unwrap();
    std::fs::remove_dir_all(root).ok();
    std::fs::remove_dir_all(
        projectless_test_root()
            .parent()
            .expect("the test root has a parent"),
    )
    .ok();
}

#[cfg(unix)]
fn serve_task_state(
    root: &std::path::Path,
    state: crate::persistence::PersistedState,
) -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = state;
    store.save(&mut state).unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        store,
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            Arc::new(backend),
            server_shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..ServerOptions::default()
            },
        )
        .unwrap()
    });
    (address, server)
}

#[cfg(unix)]
fn hydrate(client: &DaemonClient, session_id: Uuid) -> AgentSession {
    let ResponsePayload::Session {
        session: Some(session),
    } = client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::HydrateSession { session_id },
        )
        .unwrap()
    else {
        panic!("expected the stored session to hydrate");
    };
    session
}

/// The post-restart orphaned-runtime save: a session that was busy when
/// the daemon stopped is interrupted and saved while still a skeleton on
/// the client. Its column update must land without the projection's
/// placeholder workspace or empty transcript erasing stored detail.
#[cfg(unix)]
#[test]
fn a_skeleton_save_updates_columns_without_erasing_detail() {
    let root = std::env::temp_dir().join(format!("waku-skeleton-save-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mut state = crate::persistence::PersistedState::fresh(root.join("repo"));
    let session_id = state.sessions[0].id;
    let worktree = crate::model::SessionWorkspace::Worktree {
        path: root.join("repo-worktrees/task"),
        name: "task".into(),
        branch: Some("waku/task".into()),
        base_branch: None,
        adopted_by: None,
    };
    {
        let session = &mut state.sessions[0];
        session.workspace = worktree.clone();
        session.begin_turn("ship it");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        session.status = SessionStatus::Working;
        session.runtime_event_cursor = Some(crate::model::RuntimeEventCursor {
            runtime_id: Uuid::new_v4(),
            epoch: Uuid::new_v4(),
            sequence: 3,
        });
    }
    let (address, server) = serve_task_state(&root, state);
    let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();

    // The reattach path hydrates the daemon's copy before the attach
    // fails, so the merge below runs against a hydrated, busy session.
    let hydrated = hydrate(&client, session_id);
    assert_eq!(hydrated.workspace, worktree);

    let mut skeleton = hydrated.list_projection();
    skeleton.status = SessionStatus::Idle;
    client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: Vec::new(),
                live_session_ids: vec![session_id],
                sessions: vec![skeleton],
                session_tails: Vec::new(),
            },
        )
        .unwrap();

    let after = hydrate(&client, session_id);
    assert_eq!(after.status, SessionStatus::Idle);
    assert_eq!(after.workspace, worktree);
    assert_eq!(after.turns.len(), 1);

    // The stored detail survived too — this is not just in memory.
    let store = StateStore::daemon(root.join("app.db"));
    let mut stored = store.load().unwrap().sessions;
    let stored = stored
        .iter_mut()
        .find(|session| session.id == session_id)
        .unwrap();
    store.hydrate(stored).unwrap();
    assert_eq!(stored.workspace, worktree);
    assert_eq!(stored.turns.len(), 1);

    client.shutdown();
    server.join().unwrap();
    std::fs::remove_dir_all(root).ok();
}

/// Pinning or archiving a task that was never opened since launch sends
/// its skeleton: the columns must merge while the stored transcript and
/// workspace stay untouched, and an unknown skeleton creates nothing.
#[cfg(unix)]
#[test]
fn a_skeleton_save_merges_columns_and_never_creates_a_row() {
    let root = std::env::temp_dir().join(format!("waku-skeleton-pin-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mut state = crate::persistence::PersistedState::fresh(root.join("repo"));
    let session_id = state.sessions[0].id;
    let worktree = crate::model::SessionWorkspace::Worktree {
        path: root.join("repo-worktrees/task"),
        name: "task".into(),
        branch: None,
        base_branch: None,
        adopted_by: None,
    };
    {
        let session = &mut state.sessions[0];
        session.workspace = worktree.clone();
        session.begin_turn("ship it");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
    }
    let (address, server) = serve_task_state(&root, state);
    let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();

    // The daemon's copy stays a skeleton here — nothing hydrated it.
    let ResponsePayload::TaskState { sessions, .. } = client
        .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
        .unwrap()
    else {
        panic!("expected daemon task state");
    };
    let mut skeleton = sessions
        .into_iter()
        .find(|session| session.id == session_id)
        .unwrap();
    assert!(!skeleton.detail_loaded);
    skeleton.pinned_at = Some(1);
    skeleton.updated_at += 1;
    let ghost = AgentSession::new(
        crate::model::Project::from_path(root.join("repo")).id,
        ProviderKind::Codex,
    )
    .list_projection();
    let ghost_id = ghost.id;
    client
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: Vec::new(),
                live_session_ids: vec![session_id],
                sessions: vec![skeleton, ghost],
                session_tails: Vec::new(),
            },
        )
        .unwrap();

    let after = hydrate(&client, session_id);
    assert_eq!(after.pinned_at, Some(1));
    assert_eq!(after.workspace, worktree);
    assert_eq!(after.turns.len(), 1);

    // The projection of a task the daemon never stored creates no row,
    // and the stored task's skeleton still reports its worktree — the
    // sidebar's badge must not depend on a hydrate.
    let ResponsePayload::TaskState { sessions, .. } = client
        .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
        .unwrap()
    else {
        panic!("expected daemon task state");
    };
    assert!(!sessions.iter().any(|session| session.id == ghost_id));
    assert_eq!(
        sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| &session.workspace),
        Some(&worktree)
    );

    client.shutdown();
    server.join().unwrap();
    std::fs::remove_dir_all(root).ok();
}

#[cfg(unix)]
#[test]
fn a_scoped_agent_token_reaches_only_agent_commands() {
    let root = std::env::temp_dir().join(format!("goddard-agent-auth-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
    settings
        .replace(DaemonSettings {
            agent_tools_enabled: true,
            // A binary that cannot exist keeps the cold-start path from
            // ever spawning a real provider in a test.
            provider_binary_overrides: HashMap::from([(
                ProviderKind::Codex,
                "/nonexistent/waku-test-provider".into(),
            )]),
            ..DaemonSettings::default()
        })
        .unwrap();
    let backend = WakuBackend::new(settings, StateStore::daemon(root.join("app.db"))).unwrap();
    let sender_id = Uuid::new_v4();
    let agent_token = backend.agent.mint(sender_id);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            Arc::new(backend),
            server_shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..ServerOptions::default()
            },
        )
        .unwrap()
    });

    // A token the daemon never minted is no credential.
    let error = match DaemonClient::connect(&address.to_string(), "forged".into()) {
        Ok(_) => panic!("a forged token must not authenticate"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("authentication failed"),
        "{error}"
    );

    let agent = DaemonClient::connect(&address.to_string(), agent_token).unwrap();
    let human = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();

    // A target task the daemon knows but has never run.
    let project = Project::from_path(root.join("repo"));
    let mut target = AgentSession::new(project.id, ProviderKind::Codex);
    target.provider_cursor = Some(crate::model::ProviderResumeCursor::from_session_id(
        ProviderKind::Codex,
        "thread-42".into(),
    ));
    let target_id = target.id;
    human
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::SaveTaskState {
                projects: vec![project],
                live_session_ids: vec![],
                sessions: vec![target],
                session_tails: Vec::new(),
            },
        )
        .unwrap();

    // The credential is confined to the agent command surface.
    let error = agent
        .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
        .unwrap_err();
    assert!(
        error.to_string().contains("may only run agent commands"),
        "{error}"
    );

    // Unknown targets are refused rather than queued.
    let error = agent
        .request(
            sender_id,
            Uuid::nil(),
            Command::AgentPrompt {
                task_id: Some(Uuid::new_v4()),
                thread_id: None,
                provider: None,
                prompt: "hi".into(),
                delivery: crate::AgentPromptDelivery::Queue,
            },
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("unknown to the daemon"),
        "{error}"
    );

    // A provider-native thread id resolves to the same task; steer mode
    // on a task with no running turn is a clean error, not a cold start.
    let error = agent
        .request(
            sender_id,
            Uuid::nil(),
            Command::AgentPrompt {
                task_id: None,
                thread_id: Some("thread-42".into()),
                provider: None,
                prompt: "hi".into(),
                delivery: crate::AgentPromptDelivery::Steer,
            },
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("no running session to steer"),
        "{error}"
    );

    // Queue mode on a known task attempts the cold start; the fake
    // binary makes the launch itself the deterministic failure.
    let error = agent
        .request(
            sender_id,
            Uuid::nil(),
            Command::AgentPrompt {
                task_id: Some(target_id),
                thread_id: None,
                provider: None,
                prompt: "hi".into(),
                delivery: crate::AgentPromptDelivery::Queue,
            },
        )
        .unwrap_err();
    assert!(
        !error.to_string().contains("unknown to the daemon"),
        "{error}"
    );

    agent.shutdown();
    human.shutdown();
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn agent_commands_require_the_daemon_setting() {
    let root = std::env::temp_dir().join(format!("goddard-agent-gate-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    let sender_id = Uuid::new_v4();
    let agent_token = backend.agent.mint(sender_id);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            Arc::new(backend),
            server_shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..ServerOptions::default()
            },
        )
        .unwrap()
    });

    let agent = DaemonClient::connect(&address.to_string(), agent_token).unwrap();
    let human = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
    let error = agent
        .request(
            sender_id,
            Uuid::nil(),
            Command::AgentPrompt {
                task_id: Some(Uuid::new_v4()),
                thread_id: None,
                provider: None,
                prompt: "hi".into(),
                delivery: crate::AgentPromptDelivery::Queue,
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("disabled"), "{error}");

    let error = agent
        .request(
            sender_id,
            Uuid::nil(),
            Command::AgentComputerUse {
                code: "1".into(),
                timeout_ms: None,
                title: None,
            },
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("computer use is disabled"),
        "{error}"
    );
    let error = agent
        .request(Uuid::new_v4(), Uuid::nil(), Command::AgentComputerUseReset)
        .unwrap_err();
    assert!(
        error.to_string().contains("cannot target another task"),
        "{error}"
    );

    human.shutdown();
    agent.shutdown();
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn websocket_terminal_round_trip_streams_input_and_output() {
    websocket_terminal_round_trip(false);
}

/// Terminal bytes are private to the connection that opened the PTY: a
/// second client subscribed to the same channel hears nothing until it
/// issues a terminal command, which claims the channel and replays the
/// retained output tail to the claimant.
#[cfg(unix)]
#[test]
fn terminal_output_stays_with_the_owning_connection() {
    let root = std::env::temp_dir().join(format!("waku-terminal-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap()
    .with_terminal_shell(terminal_test_shell(
        "while IFS= read -r line; do printf 'received:%s\\n' \"$line\"; done",
    ));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            Arc::new(backend),
            server_shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..ServerOptions::default()
            },
        )
        .unwrap()
    });

    let owner = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
    let bystander = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
    let terminal_id = Uuid::new_v4();
    let owned = owner.subscribe(terminal_id, terminal_id);
    let tapped = bystander.subscribe(terminal_id, terminal_id);
    assert!(matches!(
        owner
            .request(
                terminal_id,
                terminal_id,
                Command::OpenTerminal {
                    cwd: root.clone(),
                    cols: 80,
                    rows: 24,
                    owner: None,
                },
            )
            .unwrap(),
        ResponsePayload::Ack
    ));
    owner
        .request(
            terminal_id,
            terminal_id,
            Command::WriteTerminal {
                data: b"first\n".to_vec(),
            },
        )
        .unwrap();
    terminal_output_until(&owned, b"received:first");
    // The whole exchange — open, write, output — stayed on the owner's
    // connection; the bystander's identical subscription got nothing.
    assert!(
        tapped.recv_timeout(Duration::from_millis(500)).is_err(),
        "a second connection received terminal events it never claimed"
    );

    // A terminal command from another connection claims the channel:
    // ownership moves and the claim answers with the retained tail.
    assert!(matches!(
        bystander
            .request(
                terminal_id,
                terminal_id,
                Command::ResizeTerminal { cols: 80, rows: 24 },
            )
            .unwrap(),
        ResponsePayload::Ack
    ));
    let repainted = terminal_output_until(&tapped, b"received:first");
    assert!(
        !repainted.is_empty(),
        "the claim did not replay the terminal's output tail"
    );
    bystander
        .request(
            terminal_id,
            terminal_id,
            Command::WriteTerminal {
                data: b"second\n".to_vec(),
            },
        )
        .unwrap();
    terminal_output_until(&tapped, b"received:second");
    // The former owner stays silent once the channel moves.
    while owned.recv_timeout(Duration::from_millis(300)).is_ok() {}
    bystander
        .request(
            terminal_id,
            terminal_id,
            Command::WriteTerminal {
                data: b"third\n".to_vec(),
            },
        )
        .unwrap();
    terminal_output_until(&tapped, b"received:third");
    assert!(
        owned.recv_timeout(Duration::from_millis(500)).is_err(),
        "the previous owner still received terminal events after the claim"
    );

    bystander
        .request(terminal_id, terminal_id, Command::CloseTerminal)
        .unwrap();
    owner.shutdown();
    bystander.shutdown();
    shutdown.store(true, Ordering::Release);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn websocket_terminal_close_does_not_wait_for_a_shell_ignoring_hangup() {
    websocket_terminal_round_trip(true);
}

#[cfg(unix)]
fn terminal_test_shell(script: &str) -> alacritty_terminal::tty::Shell {
    // Do not load the developer's or CI runner's login files, prompt
    // plugins, or terminal capability queries in a transport test.
    alacritty_terminal::tty::Shell::new("/bin/sh".into(), vec!["-c".into(), script.into()])
}

#[cfg(unix)]
fn websocket_terminal_round_trip(ignore_hangup: bool) {
    let root = std::env::temp_dir().join(format!("waku-terminal-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap()
        .with_terminal_shell(terminal_test_shell(&format!(
            "{}\nprintf 'ready:%s\\n' \"$$\"\nwhile IFS= read -r line; do printf 'received:%s\\n' \"$line\"; done",
            if ignore_hangup { "trap '' HUP" } else { ":" },
        )));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            Arc::new(backend),
            server_shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..ServerOptions::default()
            },
        )
        .unwrap()
    });

    let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
    let terminal_id = Uuid::new_v4();
    let events = client.subscribe(terminal_id, terminal_id);
    assert!(matches!(
        client
            .request(
                terminal_id,
                terminal_id,
                Command::OpenTerminal {
                    cwd: root.clone(),
                    cols: 80,
                    rows: 24,
                    owner: None,
                },
            )
            .unwrap(),
        ResponsePayload::Ack
    ));
    // Wait until the shell has installed its signal handler. Receiving
    // local echo alone does not prove that shell startup has completed.
    let ready = terminal_output_until(&events, b"\n");
    let child_pid: libc::pid_t = String::from_utf8_lossy(&ready)
        .trim()
        .strip_prefix("ready:")
        .unwrap()
        .parse()
        .unwrap();
    client
        .request(
            terminal_id,
            terminal_id,
            Command::WriteTerminal {
                data: b"waku-terminal-round-trip\r".to_vec(),
            },
        )
        .unwrap();

    // The response prefix is absent from the input, so a PTY echo cannot
    // satisfy this assertion before the child has actually read it.
    terminal_output_until(&events, b"received:waku-terminal-round-trip");

    let (closed, finished) = bounded(1);
    let closing_client = client.clone();
    let close = std::thread::spawn(move || {
        let _ =
            closed.send(closing_client.request(terminal_id, terminal_id, Command::CloseTerminal));
    });
    let result = finished.recv_timeout(Duration::from_secs(3));
    if result.is_err() {
        // Clean up the fixture even when shutdown regresses, and fail
        // here instead of waiting for the client's 120-second timeout.
        unsafe {
            libc::kill(child_pid, libc::SIGKILL);
        }
    }
    client.shutdown();
    server.join().unwrap();
    close.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        matches!(result, Ok(Ok(ResponsePayload::Ack))),
        "closing daemon terminal did not complete: {result:?}"
    );
}

#[cfg(unix)]
fn terminal_output_until(events: &Receiver<SequencedEvent>, marker: &[u8]) -> Vec<u8> {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut output = Vec::new();
    let mut seen_events = Vec::new();
    while std::time::Instant::now() < deadline
        && !output.windows(marker.len()).any(|window| window == marker)
    {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(event) = events.recv_timeout(remaining) else {
            break;
        };
        seen_events.push(event.event.kind.clone());
        if event.event.kind != "terminalOutput" {
            continue;
        }
        let data = event.event.payload["data"].as_str().unwrap();
        output.extend(
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap(),
        );
    }
    assert!(
        output.windows(marker.len()).any(|window| window == marker),
        "daemon terminal did not return the shell marker; events={seen_events:?}, output={}",
        String::from_utf8_lossy(&output)
    );
    output
}

#[cfg(unix)]
#[test]
fn paired_and_agent_tokens_cannot_change_exposure() {
    let root = std::env::temp_dir().join(format!("waku-exposure-gate-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    let agent_token = backend.agent.mint(Uuid::new_v4());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server = std::thread::spawn(move || {
        serve(
            listener,
            "secret".into(),
            Arc::new(backend),
            server_shutdown,
            ServerOptions {
                allow_shutdown: true,
                ..ServerOptions::default()
            },
        )
        .unwrap()
    });
    let owner = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();

    // A device pairs through the owner-approved flow, then connects
    // with its minted token.
    let pairing = std::thread::spawn({
        let address = address.to_string();
        move || waku_client::pair(&address, "test-device", Duration::from_secs(10))
    });
    let request_id = {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let found = match owner
                .request(Uuid::nil(), Uuid::nil(), Command::GetPairing)
                .unwrap()
            {
                ResponsePayload::Pairing { state } => {
                    state.pending.first().map(|pending| pending.request_id)
                }
                _ => None,
            };
            if let Some(request_id) = found {
                break request_id;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "pair request never arrived"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    owner
        .request(
            Uuid::nil(),
            Uuid::nil(),
            Command::RespondPairRequest {
                request_id,
                accept: true,
            },
        )
        .unwrap();
    let waku_client::PairReply::Granted { token, .. } = pairing.join().unwrap().unwrap() else {
        panic!("pair request was not granted");
    };

    let paired = DaemonClient::connect(&address.to_string(), token).unwrap();
    let agent = DaemonClient::connect(&address.to_string(), agent_token).unwrap();
    for (name, client) in [("paired", &paired), ("agent", &agent)] {
        let error = client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SetDaemonExposure { exposure: None },
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("primary authentication token")
                || error.to_string().contains("agent credential"),
            "{name}: {error}"
        );
    }

    owner.shutdown();
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

fn test_start_options() -> WireDriverStartOptions {
    WireDriverStartOptions {
        provider: "codex".into(),
        binary: PathBuf::from("codex"),
        cwd: PathBuf::from("."),
        mode: "fullAccess".into(),
        model: None,
        reasoning_effort: None,
        service_tier: None,
        context_window: None,
        agent_preset: None,
        computer_use_enabled: false,
        read_own_transcript: false,
        provider_cursor: None,
    }
}
