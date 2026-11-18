use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct ConfiguredDriver {
    options: Mutex<crate::driver::SessionOptions>,
    prompts: Mutex<Vec<Option<String>>>,
    applications: AtomicUsize,
    reject_next: AtomicBool,
    mutable: bool,
    apply_gate: Mutex<
        Option<(
            crossbeam_channel::Sender<()>,
            crossbeam_channel::Receiver<()>,
        )>,
    >,
}

impl crate::driver::DriverControl for ConfiguredDriver {
    fn prompt(&self, _: String) {
        self.prompts.lock().push(self.options.lock().model.clone());
    }
    fn cancel(&self) {}
    fn respond(&self, _: String, _: String) {}
    fn rollback(&self, _: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        Ok(None)
    }
    fn apply_options(&self, options: crate::driver::SessionOptions) -> bool {
        self.applications.fetch_add(1, Ordering::Relaxed);
        if let Some((started, release)) = self.apply_gate.lock().take() {
            started.send(()).unwrap();
            release
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }
        if !self.mutable {
            return false;
        }
        // Model selection can succeed before another option fails. The
        // owner must not keep trusting its pre-failure settings cache.
        *self.options.lock() = options;
        !self.reject_next.swap(false, Ordering::AcqRel)
    }
}

fn options(model: &str) -> crate::driver::SessionOptions {
    crate::driver::SessionOptions {
        mode: RuntimeMode::FullAccess,
        model: Some(model.into()),
        reasoning_effort: None,
        service_tier: None,
        context_window: None,
    }
}

fn configured(session_id: Uuid, runtime_id: Uuid, model: &str) -> Request {
    Request {
        request_id: Uuid::new_v4(),
        session_id,
        runtime_id,
        command: Command::PromptWithConfiguration {
            configuration: waku_protocol::PromptConfiguration {
                provider: ProviderKind::Claude,
                options: waku_protocol::model::SessionOptions {
                    mode: RuntimeMode::FullAccess,
                    model: Some(model.into()),
                    reasoning_effort: None,
                    service_tier: None,
                    context_window: None,
                },
            },
            prompt: "use the selected settings".into(),
            turn_id: None,
            message_id: None,
            hidden: false,
            attachments: Vec::new(),
            presentation: None,
        },
    }
}

#[test]
fn provider_switch_survives_reload_stale_saves_and_return_to_devin() {
    let root = std::env::temp_dir().join(format!("provider-switch-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let devin = ProviderResumeCursor::Devin {
        session_id: "devin-conversation".into(),
    };
    let source = {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .unwrap();
        backend.task_store.hydrate(session).unwrap();
        session.provider = ProviderKind::Devin;
        session.provider_cursor = Some(devin.clone());
        state.mark_session_dirty(session_id);
        backend.task_store.save(&mut state).unwrap();
        state
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .unwrap()
            .clone()
    };
    let outgoing = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        session_id,
        runtime_generation_entry(Uuid::new_v4(), outgoing.clone(), &root),
    );
    let request = |backend: &WakuBackend, command| {
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id,
                    runtime_id: Uuid::nil(),
                    command,
                },
                EventSink::detached(),
                None,
            )
            .unwrap()
    };
    let switch = |backend: &WakuBackend, source: &AgentSession, provider, resume_cursor| {
        request(
            backend,
            Command::SwitchSessionProvider {
                request: waku_protocol::ProviderSwitchRequest {
                    source: source.provider,
                    history_generation: source.history_generation,
                    boundary: source.transcript_boundary(),
                    configuration: waku_protocol::PromptConfiguration {
                        provider,
                        options: options("selected"),
                    },
                    resume_cursor,
                    handoff: "previous conversation".into(),
                },
            },
        )
    };
    let ResponsePayload::Session {
        session: Some(reply),
    } = switch(&backend, &source, ProviderKind::Codex, None)
    else {
        panic!("expected committed switch");
    };
    assert!(reply.provider_cursor.is_none());
    assert_eq!(*outgoing.shutdowns.lock(), 1);
    assert_eq!(
        reply.messages.len(),
        1,
        "switch reply carries only the notice"
    );

    for stale in [source.list_projection(), source.clone()] {
        request(
            &backend,
            Command::SaveTaskState {
                projects: Vec::new(),
                live_session_ids: vec![session_id],
                sessions: vec![stale],
                session_tails: Vec::new(),
            },
        );
    }
    drop(backend);

    let mut reopened = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    let hydrate = |backend: &WakuBackend| {
        let ResponsePayload::Session {
            session: Some(session),
        } = request(backend, Command::HydrateSession { session_id })
        else {
            panic!("expected saved session");
        };
        session
    };
    let codex = hydrate(&reopened);
    assert_eq!(codex.provider, ProviderKind::Codex);
    assert!(codex.provider_cursor.is_none());
    assert_eq!(codex.messages.len(), source.messages.len() + 1);
    assert_eq!(codex.suspended_provider_sessions[0].cursor, devin);
    assert_eq!(
        codex.suspended_provider_sessions[0].boundary,
        source.transcript_boundary()
    );
    assert_eq!(
        codex.pending_provider_context.as_deref(),
        Some("previous conversation")
    );

    runtime_generation_configure_launch(&mut reopened, &root);
    let capture = Arc::new(CaptureDriver::default());
    let launched = capture.clone();
    reopened.runtime_test_start = Some(Arc::new(move |provider, options, _| {
        assert_eq!(provider, ProviderKind::Codex);
        assert!(options.provider_cursor.is_none());
        Ok(DriverHandle::from_control(launched.clone()))
    }));
    let events = EventSink::detached();
    let (runtime_id, _) = reopened.ensure_agent_runtime(session_id, &events).unwrap();
    let mut prompt = configured(session_id, runtime_id, "selected");
    let Command::PromptWithConfiguration { configuration, .. } = &mut prompt.command else {
        unreachable!();
    };
    *configuration = waku_protocol::PromptConfiguration::for_session(&codex);
    reopened.handle(prompt, events.clone(), None).unwrap();
    assert_eq!(
        *capture.prompts.lock(),
        ["previous conversation\n\nuse the selected settings"]
    );
    for event in [
        DriverEvent::TurnStarted,
        DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None,
        },
    ] {
        record_boss_event(
            &reopened.task_state,
            &reopened.task_store,
            session_id,
            &event,
        )
        .unwrap();
    }
    let codex = hydrate(&reopened);
    assert!(codex.pending_provider_context.is_none());

    let ResponsePayload::Session {
        session: Some(reply),
    } = switch(&reopened, &codex, ProviderKind::Devin, Some(devin.clone()))
    else {
        panic!("expected committed return switch");
    };
    assert_eq!(reply.provider_cursor, Some(devin));
    assert!(hydrate(&reopened).suspended_provider_sessions.is_empty());
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn configured_prompts_validate_settings_and_reject_partial_updates_before_execution() {
    let root = std::env::temp_dir().join(format!("configured-prompts-{}", Uuid::new_v4()));
    let (mut backend, session_id) = surface_test_backend(&root);
    backend.task_state.lock().sessions[0].provider = ProviderKind::Claude;
    runtime_generation_configure_launch(&mut backend, &root);
    let runtime_id = Uuid::new_v4();
    let capture = Arc::new(ConfiguredDriver {
        options: Mutex::new(options("initial")),
        prompts: Mutex::new(Vec::new()),
        applications: AtomicUsize::new(0),
        reject_next: AtomicBool::new(false),
        mutable: true,
        apply_gate: Mutex::new(None),
    });
    let mut entry = runtime_generation_entry(runtime_id, Arc::new(CaptureDriver::default()), &root);
    entry.provider = ProviderKind::Claude;
    entry.mode = RuntimeMode::FullAccess;
    entry.driver =
        DriverHandle::from_control(capture.clone()).with_initial_options(options("initial"));
    backend.sessions.lock().insert(session_id, entry);

    // A stale runtime must not retune the replacement or accept a turn.
    let turns_before = backend.task_state.lock().sessions[0].turns.len();
    let stale = backend
        .handle(
            configured(session_id, Uuid::new_v4(), "stale"),
            EventSink::detached(),
            None,
        )
        .unwrap_err();
    assert_eq!(
        waku_protocol::RpcError::from(stale).code,
        Some(waku_protocol::RpcErrorCode::ConfigurationRejected)
    );
    assert_eq!(capture.applications.load(Ordering::Relaxed), 0);

    let mut wrong_provider = configured(session_id, runtime_id, "wrong-provider");
    if let Command::PromptWithConfiguration { configuration, .. } = &mut wrong_provider.command {
        configuration.provider = ProviderKind::Pi;
    }
    let refused = backend
        .handle(wrong_provider, EventSink::detached(), None)
        .unwrap_err();
    assert_eq!(
        waku_protocol::RpcError::from(refused).code,
        Some(waku_protocol::RpcErrorCode::ConfigurationRejected)
    );
    assert_eq!(capture.applications.load(Ordering::Relaxed), 0);

    capture.reject_next.store(true, Ordering::Release);
    let rejected = backend
        .handle(
            configured(session_id, runtime_id, "partial"),
            EventSink::detached(),
            None,
        )
        .unwrap_err();
    assert_eq!(
        waku_protocol::RpcError::from(rejected).code,
        Some(waku_protocol::RpcErrorCode::ConfigurationRejected)
    );
    assert!(capture.prompts.lock().is_empty());
    assert_eq!(
        backend.task_state.lock().sessions[0].turns.len(),
        turns_before
    );
    assert_eq!(capture.options.lock().model.as_deref(), Some("partial"));

    // Reusing the original value must validate again after partial failure.
    for model in ["initial", "latest"] {
        assert!(matches!(
            backend
                .handle(
                    configured(session_id, runtime_id, model),
                    EventSink::detached(),
                    None
                )
                .unwrap(),
            ResponsePayload::Ack
        ));
    }
    assert_eq!(
        *capture.prompts.lock(),
        [Some("initial".into()), Some("latest".into())]
    );
    assert_eq!(capture.applications.load(Ordering::Relaxed), 3);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn configured_prompt_presentation_survives_reload_without_a_desktop_save() {
    let root = std::env::temp_dir().join(format!("prompt-presentation-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    backend.task_state.lock().sessions[0].provider = ProviderKind::Claude;
    let runtime_id = Uuid::new_v4();
    let capture = Arc::new(CaptureDriver::default());
    let mut entry = runtime_generation_entry(runtime_id, capture.clone(), &root);
    entry.provider = ProviderKind::Claude;
    entry.mode = RuntimeMode::FullAccess;
    entry.driver =
        DriverHandle::from_control(capture.clone()).with_initial_options(options("initial"));
    backend.sessions.lock().insert(session_id, entry);

    let mut request = configured(session_id, runtime_id, "initial");
    let message = Uuid::new_v4();
    let atoms = vec![crate::model::MessageAtom {
        label: "pasted text".into(),
        payload: "original material".into(),
        session_id: None,
    }];
    let Command::PromptWithConfiguration {
        prompt,
        message_id,
        presentation,
        ..
    } = &mut request.command
    else {
        unreachable!()
    };
    *message_id = Some(message);
    *prompt = "expanded provider input".into();
    *presentation = Some(waku_protocol::PromptPresentation {
        content: Some("typed transcript text".into()),
        display_content: Some("displayed text".into()),
        atoms: atoms.clone(),
    });
    assert!(matches!(
        backend
            .handle(request, EventSink::detached(), None)
            .unwrap(),
        ResponsePayload::Ack
    ));
    assert_eq!(&*capture.prompts.lock(), &["expanded provider input"]);
    let reopened = StateStore::daemon(root.join("app.db"));
    let session = reopened.load_session_detail(session_id).unwrap().unwrap();
    let submitted = session.messages.iter().find(|m| m.id == message).unwrap();
    assert_eq!(submitted.content, "typed transcript text");
    assert_eq!(submitted.visible_content(), "displayed text");
    assert_eq!(submitted.atoms, atoms);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn configured_selection_survives_equal_revision_transcript_saves_and_cross_client_conflicts() {
    use waku_protocol::sidebar_intent::{SidebarEdit, SidebarIntent};
    use waku_protocol::task_metadata::TaskMetadataEdit;
    let root = std::env::temp_dir().join(format!("configured-metadata-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let selection = |model: &str| TaskMetadataEdit::Configuration {
        configuration: waku_protocol::PromptConfiguration {
            provider: ProviderKind::Codex,
            options: options(model),
        },
        auto_route: false,
        route_decision: None,
    };
    let request = |command| {
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id: Uuid::nil(),
                    runtime_id: Uuid::nil(),
                    command,
                },
                EventSink::detached(),
                None,
            )
            .unwrap()
    };
    let intent = |client_id, generation, expected: Option<TaskMetadataEdit>, edit| {
        Command::ApplySidebarIntent {
            intent: SidebarIntent {
                client_id,
                generation,
                operation_id: Uuid::new_v4(),
                expected_revision: None,
                expected: expected.map(|edit| SidebarEdit::Task {
                    id: session_id,
                    edit,
                }),
                edit: SidebarEdit::Task {
                    id: session_id,
                    edit,
                },
            },
        }
    };
    let first_client = Uuid::new_v4();
    let initial = {
        let state = backend.task_state.lock();
        TaskMetadataEdit::capture(
            waku_protocol::task_metadata::TaskMetadataField::Configuration,
            state.sessions.iter().find(|s| s.id == session_id).unwrap(),
        )
    };
    let first = intent(first_client, 1, Some(initial), selection("selected"));
    request(first.clone());
    let mut legacy = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|s| s.id == session_id)
        .unwrap()
        .clone();
    backend.task_store.hydrate(&mut legacy).unwrap();
    let revision = legacy.metadata_revision;
    request(first);
    assert_eq!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .unwrap()
            .metadata_revision,
        revision,
        "an idempotent edit advanced the revision"
    );
    // A transcript save at the current metadata revision is not a new
    // model choice. The established raw fields remain the client contract.
    legacy.push_message(crate::model::MessageRole::Assistant, "transcript progress");
    legacy.updated_at += 1;
    request(Command::SaveTaskState {
        projects: Vec::new(),
        live_session_ids: Vec::new(),
        sessions: vec![legacy],
        session_tails: Vec::new(),
    });
    assert_eq!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .unwrap()
            .model
            .as_deref(),
        Some("selected")
    );
    request(intent(
        Uuid::new_v4(),
        1,
        Some(selection("selected")),
        selection("other-client"),
    ));
    // A later local edit based on a value replaced elsewhere is explicitly
    // blocked by the existing metadata owner; it must not erase that edit.
    request(intent(
        first_client,
        2,
        Some(selection("selected")),
        selection("conflicting"),
    ));
    assert_eq!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .unwrap()
            .model
            .as_deref(),
        Some("other-client")
    );
    let mut restored = backend.task_store.load().unwrap();
    let session = restored
        .sessions
        .iter_mut()
        .find(|s| s.id == session_id)
        .unwrap();
    assert_eq!(session.model.as_deref(), Some("other-client"));
    backend.task_store.hydrate(session).unwrap();
    assert_eq!(session.model.as_deref(), Some("other-client"));
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn configured_selection_keeps_legacy_clients_able_to_change_the_shared_model() {
    use waku_protocol::task_metadata::{TaskMetadataEdit, TaskMetadataUpdate};
    let root = std::env::temp_dir().join(format!("configured-legacy-client-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let request = |command| {
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id: Uuid::nil(),
                    runtime_id: Uuid::nil(),
                    command,
                },
                EventSink::detached(),
                None,
            )
            .unwrap()
    };
    request(Command::UpdateTaskMetadata {
        updates: vec![TaskMetadataUpdate {
            session_id,
            edit: TaskMetadataEdit::Configuration {
                configuration: waku_protocol::PromptConfiguration {
                    provider: ProviderKind::Codex,
                    options: options("first-client-model"),
                },
                auto_route: false,
                route_decision: None,
            },
        }],
    });
    let mut legacy = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|s| s.id == session_id)
        .unwrap()
        .clone();
    backend.task_store.hydrate(&mut legacy).unwrap();
    // Web/mobile and older clients edit the established fields and save them.
    // Their saved model change must remain a supported user action.
    legacy.model = Some("legacy-client-model".into());
    legacy.updated_at += 1;
    request(Command::SaveTaskState {
        projects: Vec::new(),
        live_session_ids: Vec::new(),
        sessions: vec![legacy],
        session_tails: Vec::new(),
    });
    assert_eq!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .unwrap()
            .model
            .as_deref(),
        Some("legacy-client-model"),
        "a supported legacy settings edit was silently ignored"
    );
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn configured_prompt_keeps_a_fresh_immutable_provider_and_preserves_permission_posture() {
    let root = std::env::temp_dir().join(format!("configured-immutable-{}", Uuid::new_v4()));
    let (mut backend, session_id) = surface_test_backend(&root);
    backend.task_state.lock().sessions[0].provider = ProviderKind::Claude;
    runtime_generation_configure_launch(&mut backend, &root);
    let runtime_id = Uuid::new_v4();
    let capture = Arc::new(ConfiguredDriver {
        options: Mutex::new(options("initial")),
        prompts: Mutex::new(Vec::new()),
        applications: AtomicUsize::new(0),
        reject_next: AtomicBool::new(false),
        mutable: false,
        apply_gate: Mutex::new(None),
    });
    let mut entry = runtime_generation_entry(runtime_id, Arc::new(CaptureDriver::default()), &root);
    entry.provider = ProviderKind::Claude;
    entry.mode = RuntimeMode::FullAccess;
    entry.driver =
        DriverHandle::from_control(capture.clone()).with_initial_options(options("initial"));
    backend.sessions.lock().insert(session_id, entry);
    backend
        .handle(
            configured(session_id, runtime_id, "initial"),
            EventSink::detached(),
            None,
        )
        .unwrap();
    assert_eq!(capture.applications.load(Ordering::Relaxed), 0);
    assert_eq!(capture.prompts.lock().len(), 1);

    let mut wrong_mode = configured(session_id, runtime_id, "initial");
    if let Command::PromptWithConfiguration { configuration, .. } = &mut wrong_mode.command {
        configuration.options.mode = RuntimeMode::Ask;
    }
    let rejected = backend
        .handle(wrong_mode, EventSink::detached(), None)
        .unwrap_err();
    assert_eq!(
        waku_protocol::RpcError::from(rejected).code,
        Some(waku_protocol::RpcErrorCode::ConfigurationRejected)
    );
    assert_eq!(capture.applications.load(Ordering::Relaxed), 0);
    let rejected = backend
        .handle(
            configured(session_id, runtime_id, "different"),
            EventSink::detached(),
            None,
        )
        .unwrap_err();
    assert_eq!(
        waku_protocol::RpcError::from(rejected).code,
        Some(waku_protocol::RpcErrorCode::ConfigurationRejected)
    );
    assert_eq!(capture.prompts.lock().len(), 1);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_held_configuration_blocks_its_prompt_but_not_another_task_or_health() {
    let root = std::env::temp_dir().join(format!("configured-held-{}", Uuid::new_v4()));
    let (mut backend, session_id) = surface_test_backend(&root);
    backend.task_state.lock().sessions[0].provider = ProviderKind::Claude;
    runtime_generation_configure_launch(&mut backend, &root);
    let runtime_id = Uuid::new_v4();
    let (started, started_rx) = crossbeam_channel::bounded(1);
    let (release, release_rx) = crossbeam_channel::bounded(1);
    let capture = Arc::new(ConfiguredDriver {
        options: Mutex::new(options("initial")),
        prompts: Mutex::new(Vec::new()),
        applications: AtomicUsize::new(0),
        reject_next: AtomicBool::new(false),
        mutable: true,
        apply_gate: Mutex::new(Some((started, release_rx))),
    });
    let mut entry = runtime_generation_entry(runtime_id, Arc::new(CaptureDriver::default()), &root);
    entry.provider = ProviderKind::Claude;
    entry.mode = RuntimeMode::FullAccess;
    entry.driver =
        DriverHandle::from_control(capture.clone()).with_initial_options(options("initial"));
    backend.sessions.lock().insert(session_id, entry);
    let sibling = Uuid::new_v4();
    let sibling_runtime = Uuid::new_v4();
    let mut sibling_entry =
        runtime_generation_entry(sibling_runtime, Arc::new(CaptureDriver::default()), &root);
    sibling_entry.provider = ProviderKind::Claude;
    backend.sessions.lock().insert(sibling, sibling_entry);

    let (prompt, samples, premature, probes_finished) = std::thread::scope(|scope| {
        let backend = &backend;
        let prompt = scope.spawn(|| {
            backend.handle(
                configured(session_id, runtime_id, "selected"),
                EventSink::detached(),
                None,
            )
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let premature = !capture.prompts.lock().is_empty();
        let (finished, finished_rx) = crossbeam_channel::bounded(1);
        let probes = scope.spawn(move || {
            let samples = (0..30)
                .map(|_| {
                    let began = std::time::Instant::now();
                    let health = backend.handle(
                        Request {
                            request_id: Uuid::new_v4(),
                            session_id: Uuid::nil(),
                            runtime_id: Uuid::nil(),
                            command: Command::GetDaemonStats,
                        },
                        EventSink::detached(),
                        None,
                    );
                    let navigation = backend.handle(
                        Request {
                            request_id: Uuid::new_v4(),
                            session_id: sibling,
                            runtime_id: sibling_runtime,
                            command: Command::AttachSession,
                        },
                        EventSink::detached(),
                        None,
                    );
                    (began.elapsed(), health.is_ok(), navigation.is_ok())
                })
                .collect::<Vec<_>>();
            let _ = finished.send(());
            samples
        });
        // Release before joining on failure: a leaked global lock must fail
        // the isolation assertion, not deadlock the fixture's own cleanup.
        let probes_finished = finished_rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .is_ok();
        release.send(()).unwrap();
        (
            prompt.join().unwrap(),
            probes.join().unwrap(),
            premature,
            probes_finished,
        )
    });
    assert!(matches!(prompt.unwrap(), ResponsePayload::Ack));
    assert!(
        !premature,
        "the provider ran before its settings were released"
    );
    assert!(
        probes_finished,
        "unrelated work waited behind the settings dependency"
    );
    assert_eq!(*capture.prompts.lock(), [Some("selected".into())]);
    for (elapsed, health, navigation) in &samples {
        assert!(*health && *navigation, "an unrelated request failed");
        assert!(
            *elapsed < std::time::Duration::from_millis(250),
            "unrelated work waited {elapsed:?}"
        );
    }
    let mut times: Vec<_> = samples.iter().map(|(elapsed, _, _)| *elapsed).collect();
    times.sort();
    eprintln!(
        "held configuration: 30 health+task-open probes p95={:?} max={:?}",
        times[28], times[29]
    );
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}
