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
        Self::with_handler(|_| None)
    }

    fn with_handler(
        handler: impl Fn(&Command) -> Option<ResponsePayload> + Send + 'static,
    ) -> Self {
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
                            _ => handler(&request.command).unwrap_or_else(|| {
                                match &request.command {
                                    Command::SaveTaskState { sessions, .. } => {
                                        ResponsePayload::TaskStateSaved {
                                            sessions: sessions.clone(),
                                            receipts: Some(
                                                sessions
                                                    .iter()
                                                    .map(|session| {
                                                        waku_protocol::SessionSaveReceipt {
                                        session_id: session.id,
                                        detail: waku_protocol::SessionDetailSaveOutcome::Applied,
                                    }
                                                    })
                                                    .collect(),
                                            ),
                                        }
                                    }
                                    _ => ResponsePayload::Ack,
                                }
                            }),
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

fn transcript_match(relevance: u32) -> SessionMessageMatch {
    SessionMessageMatch {
        session_id: Uuid::new_v4(),
        message_id: Uuid::new_v4(),
        source: crate::model::MessageRole::Assistant,
        snippet: "needle".into(),
        relevance: Some(relevance),
        session_updated_at: Some(1),
    }
}

#[test]
fn transcript_search_publishes_fast_hosts_and_merges_before_limiting() {
    let weak = transcript_match(1);
    let strong = transcript_match(100);
    let strong_id = strong.session_id;
    let (release, gate) = unbounded();
    let local = DaemonFixture::with_handler(move |command| {
        if matches!(command, Command::SearchSessionMessages { .. }) {
            gate.recv_timeout(Duration::from_secs(10)).unwrap();
            Some(ResponsePayload::SessionMessageMatches {
                matches: vec![weak.clone()],
            })
        } else {
            None
        }
    });
    let remote = DaemonFixture::with_handler(move |command| {
        matches!(command, Command::SearchSessionMessages { .. }).then(|| {
            ResponsePayload::SessionMessageMatches {
                matches: vec![strong.clone()],
            }
        })
    });
    let fixture = StoreFixture::new(&local);
    fixture
        .store
        .daemons
        .add_remote(Uuid::new_v4(), remote.supervisor.clone());
    let (updates_tx, updates_rx) = unbounded();
    let search = fixture.store.session_message_search_with_updates(
        "needle".into(),
        1,
        SessionMessageSearchScope::Active,
        None,
        move |update| {
            updates_tx.send(update).unwrap();
        },
    );
    let worker = std::thread::spawn(search);
    let first = updates_rx.recv_timeout(Duration::from_secs(5));
    // Release before asserting, so a sequential-search regression cannot hang cleanup.
    release.send(()).unwrap();
    let final_hits = worker.join().unwrap().unwrap();
    let first = first.expect("the fast remote must publish while the local host is blocked");
    assert_eq!(first.pending, 1);
    assert_eq!(first.matches[0].session_id, strong_id);
    assert_eq!(final_hits.len(), 1);
    assert_eq!(final_hits[0].session_id, strong_id);
}

#[test]
fn transcript_search_reports_partial_total_and_disconnected_host_failures() {
    // An unexpected payload is a failed search, never a successful empty result.
    let failing = DaemonFixture::new();
    let hit = transcript_match(100);
    let good = DaemonFixture::with_handler(move |command| {
        matches!(command, Command::SearchSessionMessages { .. }).then(|| {
            ResponsePayload::SessionMessageMatches {
                matches: vec![hit.clone()],
            }
        })
    });
    let fixture = StoreFixture::new(&failing);
    let host = Uuid::new_v4();
    fixture
        .store
        .daemons
        .add_remote(host, good.supervisor.clone());
    let (tx, rx) = unbounded();
    let hits = fixture.store.session_message_search_with_updates(
        "needle".into(),
        10,
        SessionMessageSearchScope::Active,
        None,
        move |update| {
            tx.send(update).unwrap();
        },
    )()
    .unwrap();
    assert_eq!(hits.len(), 1);
    let settled = rx.iter().last().unwrap();
    assert_eq!((settled.pending, settled.failed, settled.total), (0, 1, 2));
    fixture.store.daemons.detach_remote(host);
    let (tx, rx) = unbounded();
    assert!(
        fixture.store.session_message_search_with_updates(
            "needle".into(),
            10,
            SessionMessageSearchScope::Active,
            None,
            move |update| {
                tx.send(update).unwrap();
            },
        )()
        .is_err()
    );
    let settled = rx.iter().last().unwrap();
    assert_eq!((settled.pending, settled.failed, settled.total), (0, 1, 1));
    let remote_task = Uuid::new_v4();
    fixture
        .store
        .daemons
        .claim_session(remote_task, DaemonKey::Remote(host));
    let (tx, rx) = unbounded();
    assert!(
        fixture.store.session_message_search_with_updates(
            "needle".into(),
            10,
            SessionMessageSearchScope::Active,
            Some(vec![remote_task]),
            move |update| {
                tx.send(update).unwrap();
            },
        )()
        .is_err()
    );
    let settled = rx.iter().last().unwrap();
    assert_eq!((settled.pending, settled.failed, settled.total), (0, 1, 1));
}

#[test]
fn task_saves_coalesce_without_blocking_edits_and_wait_for_acknowledgements() {
    let (entered, observed) = unbounded();
    let (release, gate) = unbounded();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let local = DaemonFixture::with_handler(move |command| {
        if let Command::SaveTaskState { sessions, .. } = command {
            let index = calls.fetch_add(1, Ordering::SeqCst);
            entered.send(index).unwrap();
            if index < 2 {
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            return Some(ResponsePayload::TaskStateSaved {
                sessions: sessions.clone(),
                receipts: Some(
                    sessions
                        .iter()
                        .map(|session| waku_protocol::SessionSaveReceipt {
                            session_id: session.id,
                            detail: waku_protocol::SessionDetailSaveOutcome::Applied,
                        })
                        .collect(),
                ),
            });
        }
        None
    });
    let fixture = StoreFixture::new(&local);
    let store = &fixture.store;
    let mut state = PersistedState::empty();
    let project = Project::from_path(fixture.root.join("project"));
    let mut session = AgentSession::new(project.id, ProviderKind::Codex);
    session.begin_turn("initial prompt");
    session.detail_loaded = true;
    let id = session.id;
    state.projects.push(project);
    state.push_session(session);

    store.queue_save(&mut state).unwrap();
    assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
    assert!(state.trim_idle_transcripts(&HashSet::new(), 0).is_empty());
    for title in ["intermediate", "latest"] {
        state.session_mut(id).unwrap().title = title.into();
        store.queue_save(&mut state).unwrap();
    }
    let newest = state.pending_session_saves[&id];
    release.send(()).unwrap();
    assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
    store.collect_task_saves(&mut state);
    assert_eq!(
        state.pending_session_saves[&id], newest,
        "old ack cleared a newer save"
    );
    assert!(state.trim_idle_transcripts(&HashSet::new(), 0).is_empty());
    // A further edit made while the second save is in flight must stay dirty
    // even when that save completes successfully.
    state.session_mut(id).unwrap().title = "unsaved edit".into();
    let tickets = store.task_save_tickets();
    release.send(()).unwrap();
    smol::block_on(async {
        for ticket in tickets {
            ticket.wait().await.unwrap();
        }
    });
    store.collect_task_saves(&mut state);
    assert!(state.dirty_sessions.contains(&id));
    assert!(!state.pending_session_saves.contains_key(&id));
    let saves = [local.next_request(), local.next_request()];
    let Command::SaveTaskState { sessions, .. } = &saves[1].command else {
        panic!("expected save")
    };
    assert_eq!(sessions[0].title, "latest");
    assert!(
        local.requests.is_empty(),
        "obsolete intermediate snapshot was sent"
    );
}

#[test]
fn unconfirmed_task_save_keeps_data_dirty_until_a_confirmed_retry() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let local = DaemonFixture::with_handler(move |command| {
        if matches!(command, Command::SaveTaskState { .. })
            && calls.fetch_add(1, Ordering::SeqCst) == 0
        {
            return Some(ResponsePayload::Ack);
        }
        None
    });
    let fixture = StoreFixture::new(&local);
    let store = &fixture.store;
    let mut state = PersistedState::empty();
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("must survive an unconfirmed save");
    let id = session.id;
    state.push_session(session);
    assert!(store.save(&mut state).is_err());
    assert!(state.dirty_sessions.contains(&id));
    assert!(state.pending_session_saves.contains_key(&id));
    store.save(&mut state).unwrap();
    assert!(!state.dirty_sessions.contains(&id));
    assert!(!state.pending_session_saves.contains_key(&id));
}

#[test]
fn rejected_task_tail_retries_full_before_advancing_the_save_cursor() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let local = DaemonFixture::with_handler(move |command| {
        if let Command::SaveTaskState {
            sessions,
            session_tails,
            ..
        } = command
        {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            if call == 1 {
                assert_eq!(session_tails.len(), 1);
            } else {
                assert!(session_tails.is_empty());
            }
            return Some(ResponsePayload::TaskStateSaved {
                sessions: sessions.clone(),
                receipts: Some(
                    sessions
                        .iter()
                        .map(|session| waku_protocol::SessionSaveReceipt {
                            session_id: session.id,
                            detail: if call == 1 {
                                waku_protocol::SessionDetailSaveOutcome::FullRequired
                            } else {
                                waku_protocol::SessionDetailSaveOutcome::Applied
                            },
                        })
                        .collect(),
                ),
            });
        }
        None
    });
    let fixture = StoreFixture::new(&local);
    let store = &fixture.store;
    let mut state = PersistedState::empty();
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("original prompt");
    session.push_message(
        waku_protocol::model::MessageRole::Assistant,
        "original answer",
    );
    session.detail_loaded = true;
    let id = session.id;
    state.push_session(session);
    store.save(&mut state).unwrap();
    state
        .session_mut(id)
        .unwrap()
        .push_message(waku_protocol::model::MessageRole::User, "new question");
    store.save(&mut state).unwrap();
    assert!(!state.pending_session_saves.contains_key(&id));
    for _ in 0..2 {
        local.next_request();
    }
    let Command::SaveTaskState {
        sessions,
        session_tails,
        ..
    } = local.next_request().command
    else {
        panic!("expected save")
    };
    assert!(session_tails.is_empty());
    assert_eq!(sessions[0].messages.len(), state.sessions[0].messages.len());
    assert_eq!(sessions[0].messages.last().unwrap().content, "new question");
}

#[test]
fn slow_settings_rpc_coalesces_edits_and_preserves_pending_credentials() {
    use waku_protocol::inference::InferenceProvider;
    let (entered, observed) = unbounded();
    let (release, gate) = unbounded();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let local = DaemonFixture::with_handler(move |command| {
        if matches!(command, Command::UpdateSettings { .. }) {
            let index = calls.fetch_add(1, Ordering::SeqCst);
            entered.send(index).unwrap();
            if index == 0 {
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            }
        }
        None
    });
    let provider = InferenceProvider::OpenRouter;
    let mut settings = DaemonSettings::default();
    settings
        .inference
        .entry(provider)
        .or_default()
        .config
        .insert("route".into(), "initial".into());
    local.supervisor.update_settings(settings.clone()).unwrap();
    assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
    settings.inference.get_mut(&provider).unwrap().api_key = Some("fixture-credential".into());
    local.supervisor.update_settings(settings.clone()).unwrap();
    settings.inference.get_mut(&provider).unwrap().api_key = None;
    for index in 0..1000 {
        settings
            .inference
            .get_mut(&provider)
            .unwrap()
            .config
            .insert("route".into(), index.to_string());
        local.supervisor.update_settings(settings.clone()).unwrap();
    }
    release.send(()).unwrap();
    assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
    local.next_request();
    let Command::UpdateSettings { settings } = local.next_request().command else {
        panic!("expected settings")
    };
    assert_eq!(settings.inference[&provider].config["route"], "999");
    assert_eq!(
        settings.inference[&provider].api_key.as_deref(),
        Some("fixture-credential")
    );
    assert!(local.requests.is_empty(), "superseded settings were sent");
}
