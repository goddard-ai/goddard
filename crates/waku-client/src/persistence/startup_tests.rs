use super::*;
use crate::{ClientMessage, PROTOCOL_VERSION, ResponseOutcome, ServerMessage};

#[test]
fn startup_catalog_load_does_not_request_selected_transcript() {
    let root = std::env::temp_dir().join(format!("startup-catalog-{}", Uuid::new_v4()));
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let project = Project::from_path(root.clone());
    let mut session = AgentSession::new(project.id, ProviderKind::Codex);
    session.begin_turn("Stored prompt");
    let selected = session.id;
    let mut preferences = PersistedState::empty();
    preferences.selected_session = Some(selected);
    preferences.selected_project = Some(project.id);
    preferences.sessions.push(session.clone());
    preferences.projects.push(project.clone());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let server_root = root.clone();
    let projectless_root = waku_protocol::projectless::workspace_root();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        let mut hydrations = 0;
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
                    let payload = match request.command {
                        Command::GetSettings => ResponsePayload::Settings {
                            settings: DaemonSettings::default(),
                        },
                        Command::LoadTaskState => ResponsePayload::TaskState {
                            projects: vec![project.clone()],
                            sessions: vec![session.list_projection()],
                            default_cwd: server_root.clone(),
                            projectless_root: projectless_root.clone(),
                        },
                        Command::HydrateSession { .. } => {
                            hydrations += 1;
                            ResponsePayload::Session { session: None }
                        }
                        _ => ResponsePayload::Ack,
                    };
                    ServerMessage::Response {
                        request_id: request.request_id,
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
        hydrations
    });
    let daemon = DaemonSupervisor::connect(&address, "test-token".into()).unwrap();
    let mut store = StateStore::remote(daemon.clone());
    // Keep all preference reads and writes inside this fixture.
    store.app_state_path = root.join("app-state.json");
    store.app_settings_path = root.join("app-settings.json");
    store.legacy_settings_paths.clear();
    store.write_app_state(&preferences.app_state()).unwrap();

    let state = store.load_or_fresh(root.clone());
    daemon.client().force_disconnect();
    let hydrations = server.join().unwrap();
    let loaded = state
        .sessions
        .iter()
        .find(|session| session.id == selected)
        .unwrap();
    assert_eq!(state.selected_session, Some(selected));
    assert!(!loaded.detail_loaded);
    assert_eq!(
        hydrations, 0,
        "catalog loading must not wait for task detail"
    );
}
