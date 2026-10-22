use super::*;
use crate::{ClientMessage, PROTOCOL_VERSION, Request, ResponseOutcome, ServerMessage};
use crossbeam_channel::{Receiver, unbounded};
use std::time::Duration;

/// Real client transport; only the external daemon is replaced. Every command
/// is recorded so tests can prove which host received a destructive write.
struct DaemonFixture {
    supervisor: DaemonSupervisor,
    requests: Receiver<Request>,
    server: Option<std::thread::JoinHandle<()>>,
}

impl DaemonFixture {
    fn new() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let (requests, received) = unbounded();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            drop(listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            while let Ok(message) = socket.read() {
                let tungstenite::Message::Text(text) = message else {
                    continue;
                };
                let response = match serde_json::from_str::<ClientMessage>(&text).unwrap() {
                    ClientMessage::Hello { .. } => ServerMessage::Hello {
                        protocol_version: PROTOCOL_VERSION,
                        daemon_version: "test".into(),
                        daemon_commit: None,
                        agent_cli_available: false,
                    },
                    ClientMessage::Request(request) => {
                        let payload = match &request.command {
                            Command::GetSettings => ResponsePayload::Settings {
                                settings: DaemonSettings::default(),
                            },
                            _ => ResponsePayload::Ack,
                        };
                        let request_id = request.request_id;
                        requests.send(request).unwrap();
                        if request_id.is_nil() {
                            continue;
                        }
                        ServerMessage::Response {
                            request_id,
                            outcome: ResponseOutcome::Ok { payload },
                        }
                    }
                    _ => break,
                };
                if socket
                    .send(tungstenite::Message::Text(
                        serde_json::to_string(&response).unwrap().into(),
                    ))
                    .is_err()
                {
                    break;
                }
            }
        });
        let supervisor = DaemonSupervisor::connect(&address, "test-token".into()).unwrap();
        assert!(matches!(
            received
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .command,
            Command::GetSettings
        ));
        Self {
            supervisor,
            requests: received,
            server: Some(server),
        }
    }

    fn next_request(&self) -> Request {
        self.requests.recv_timeout(Duration::from_secs(2)).unwrap()
    }
}

impl Drop for DaemonFixture {
    fn drop(&mut self) {
        self.supervisor.client().force_disconnect();
        self.server.take().unwrap().join().unwrap();
    }
}

struct StoreFixture {
    store: StateStore,
    root: PathBuf,
}

impl StoreFixture {
    fn new(local: &DaemonFixture) -> Self {
        let root = std::env::temp_dir().join(format!("remote-state-{}", Uuid::new_v4()));
        let mut store = StateStore::remote(local.supervisor.clone());
        store.app_state_path = root.join("app-state.json");
        store.app_settings_path = root.join("app-settings.json");
        store.legacy_settings_paths.clear();
        // Model an already loaded local catalog; remote catalogs are gated
        // separately and must never be assumed loaded merely by connecting.
        store.task_state_loaded.store(true, Ordering::Release);
        Self { store, root }
    }
}

impl Drop for StoreFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn failed_remote_deletion_keeps_ownership_so_retry_cannot_reach_local() {
    let local = DaemonFixture::new();
    let fixture = StoreFixture::new(&local);
    let store = &fixture.store;
    let host = Uuid::new_v4();
    let session = Uuid::new_v4();
    let owner = DaemonKey::Remote(host);
    store.daemons.replace_remote_catalog(host, &[], &[session]);

    // Cached remote catalog, with no connection yet.
    assert!(
        store.remove_session(session).is_err(),
        "offline deletion reported success"
    );
    assert_eq!(store.daemons.session_owner(session), owner);

    let remote = DaemonFixture::new();
    store.daemons.add_remote(host, remote.supervisor.clone());
    remote.supervisor.client().force_disconnect();
    assert!(store.remove_session(session).is_err());
    assert_eq!(store.daemons.session_owner(session), owner);
    assert!(store.remove_session(session).is_err());
    assert_eq!(store.daemons.session_owner(session), owner);
    assert!(
        local.requests.is_empty(),
        "remote deletion leaked to local daemon"
    );
}

#[test]
fn merged_saves_are_isolated_and_unloaded_remote_catalogs_are_never_written() {
    let local = DaemonFixture::new();
    let remote = DaemonFixture::new();
    let fixture = StoreFixture::new(&local);
    let store = &fixture.store;
    let host = Uuid::new_v4();
    let offline_host = Uuid::new_v4();
    store.daemons.add_remote(host, remote.supervisor.clone());
    let mut state = PersistedState::empty();
    for (index, owner) in [
        DaemonKey::Local,
        DaemonKey::Remote(host),
        DaemonKey::Remote(offline_host),
    ]
    .into_iter()
    .enumerate()
    {
        let project = Project::from_path(fixture.root.join(index.to_string()));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.begin_turn("A task belonging to one host");
        store.daemons.claim_project(project.id, owner);
        store.daemons.claim_session(session.id, owner);
        state.dirty_sessions.insert(session.id);
        state.projects.push(project);
        state.sessions.push(session);
    }

    store.save(&mut state).unwrap();
    assert!(
        remote.requests.is_empty(),
        "connecting is not permission to replace a remote catalog"
    );
    assert_eq!(
        state.dirty_sessions,
        HashSet::from([state.sessions[1].id, state.sessions[2].id])
    );
    let local_save = local.next_request();
    let remote_project = state.projects[1].id;
    let remote_session = state.sessions[1].id;
    store
        .daemons
        .replace_remote_catalog(host, &[remote_project], &[remote_session]);
    store.save(&mut state).unwrap();
    let remote_save = remote.next_request();
    for (request, index) in [(local_save, 0), (remote_save, 1)] {
        let Command::SaveTaskState {
            projects,
            live_session_ids,
            sessions,
            ..
        } = request.command
        else {
            panic!("expected save");
        };
        assert_eq!(
            projects
                .iter()
                .map(|project| project.id)
                .collect::<Vec<_>>(),
            [state.projects[index].id]
        );
        assert_eq!(live_session_ids, [state.sessions[index].id]);
        assert_eq!(
            sessions
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            [state.sessions[index].id]
        );
    }
    assert_eq!(state.dirty_sessions, HashSet::from([state.sessions[2].id]));
}
