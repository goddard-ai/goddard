use super::*;

fn next(events: &crossbeam_channel::Receiver<waku_protocol::ServerMessage>) -> DriverEvent {
    loop {
        if let waku_protocol::ServerMessage::Event(event) = events
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
        {
            return event_from_wire(event.event).unwrap();
        }
    }
}

#[test]
fn task_views_handoff_recovers_hidden_output_and_fences_stale_saves() {
    let root = std::env::temp_dir().join(format!("task-view-handoff-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let runtime_id = Uuid::new_v4();
    let sink = EventSink::detached().begin_session_runtime(session_id, runtime_id);
    let (client, events) = sink.tapped_connection();
    let (sender, receiver) = driver::test_event_channel();
    let driver = Arc::new(CaptureDriver::default());
    let forwarder = backend
        .register_runtime_forwarder(
            session_id,
            runtime_generation_entry(runtime_id, driver.clone(), &root),
            receiver,
            sink,
        )
        .unwrap();
    let view = |detail, generation| {
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id,
                    runtime_id,
                    command: Command::SetTaskView {
                        detail,
                        generation,
                        payloads: Default::default(),
                    },
                },
                client.clone(),
                None,
            )
            .unwrap()
    };
    view(false, 1);
    assert!(matches!(
        next(&events),
        DriverEvent::TaskViewSnapshot { detail: false, .. }
    ));
    let turn_id = Uuid::new_v4();
    backend
        .handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id,
                runtime_id,
                command: Command::Prompt {
                    runtime_mode: None,
                    prompt: "go".into(),
                    turn_id: Some(turn_id),
                    message_id: Some(Uuid::new_v4()),
                    hidden: false,
                    attachments: Vec::new(),
                },
            },
            client.clone(),
            None,
        )
        .unwrap();
    sender.send(DriverEvent::TurnStarted).unwrap();
    sender
        .send(DriverEvent::TextDelta("unseen answer".into()))
        .unwrap();
    sender
        .send(DriverEvent::Permission {
            request_id: "permission-1".into(),
            title: "Allow?".into(),
            title_i18n: None,
            detail: String::new(),
            detail_i18n: None,
            options: Vec::new(),
        })
        .unwrap();
    assert!(matches!(next(&events), DriverEvent::PromptSubmitted { .. }));
    assert!(matches!(next(&events), DriverEvent::TurnStarted));
    assert!(matches!(next(&events), DriverEvent::Permission { .. }));
    assert!(events.try_recv().is_err());
    view(true, 3);
    let DriverEvent::TaskViewSnapshot {
        session,
        detail: true,
        ..
    } = next(&events)
    else {
        panic!("snapshot must precede live output")
    };
    assert!(
        session
            .messages
            .iter()
            .any(|message| message.content == "unseen answer")
    );
    assert_eq!(session.active_turn_id(), Some(turn_id));
    assert!(
        matches!(next(&events), DriverEvent::Permission { request_id, .. } if request_id == "permission-1")
    );
    sender
        .send(DriverEvent::RequestSettled {
            request_id: "permission-1".into(),
        })
        .unwrap();
    assert!(matches!(next(&events), DriverEvent::RequestSettled { .. }));
    view(false, 2); // Delayed navigation cannot cancel the newer observation.
    sender
        .send(DriverEvent::TextDelta(" continues".into()))
        .unwrap();
    assert!(matches!(next(&events), DriverEvent::TextDelta(text) if text == " continues"));

    let mut queued = crate::model::QueuedMessage::new("also this");
    queued.display_content = Some("steer presentation".into());
    let steer = || {
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id,
                    runtime_id,
                    command: Command::Steer {
                        prompt: queued.content.clone(),
                        hidden: false,
                        queued_message: Some(queued.clone()),
                    },
                },
                client.clone(),
                None,
            )
            .unwrap()
    };
    let before = driver.steers.lock().len();
    steer();
    steer();
    assert_eq!(driver.steers.lock().len(), before + 1);
    sender
        .send(DriverEvent::SteerAccepted {
            message: queued.content.clone(),
            sent_by_task: None,
            hidden: false,
            report_trigger: None,
        })
        .unwrap();
    assert!(matches!(next(&events), DriverEvent::SteerAccepted { .. }));
    steer();
    assert_eq!(driver.steers.lock().len(), before + 1);
    {
        let state = backend.task_state.lock();
        let task = state
            .sessions
            .iter()
            .find(|task| task.id == session_id)
            .unwrap();
        let accepted = task
            .messages
            .iter()
            .find(|message| message.id == queued.id)
            .unwrap();
        assert_eq!(
            accepted.display_content.as_deref(),
            Some("steer presentation")
        );
        assert!(
            !task
                .queued_messages
                .iter()
                .any(|message| message.id == queued.id)
        );
    }

    let mut stale = *session;
    stale.messages.clear();
    backend
        .handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id,
                runtime_id,
                command: Command::SaveTaskState {
                    projects: Vec::new(),
                    live_session_ids: vec![session_id],
                    sessions: vec![stale],
                    session_tails: Vec::new(),
                },
            },
            client.clone(),
            None,
        )
        .unwrap();
    view(true, 4);
    let DriverEvent::TaskViewSnapshot { session, .. } = next(&events) else {
        panic!("missing refreshed snapshot")
    };
    assert!(
        session
            .messages
            .iter()
            .any(|message| message.content == "unseen answer continues")
    );
    view(false, 5);
    assert!(matches!(
        next(&events),
        DriverEvent::TaskViewSnapshot { detail: false, .. }
    ));
    sender
        .send(DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None,
        })
        .unwrap();
    assert!(matches!(
        next(&events),
        DriverEvent::TurnFinished { success: true, .. }
    ));
    drop(sender);
    forwarder.join().unwrap();
    assert!(
        backend.sessions.lock().contains_key(&session_id),
        "unobserve must not close the provider"
    );
    let removed = backend.remove_runtime(session_id, Some(runtime_id), false);
    drop(removed);
    let mut stale = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap()
        .clone();
    stale.messages.clear();
    let save = |session| {
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id,
                    runtime_id,
                    command: Command::SaveTaskState {
                        projects: Vec::new(),
                        live_session_ids: vec![session_id],
                        sessions: vec![session],
                        session_tails: Vec::new(),
                    },
                },
                client.clone(),
                None,
            )
            .unwrap()
    };
    save(stale);
    let mut prepared = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap()
        .clone();
    assert!(
        prepared
            .messages
            .iter()
            .any(|message| message.content == "unseen answer continues"),
        "a final save after process exit cannot erase hidden output"
    );
    let new_turn = prepared.begin_turn_with_presentation(
        "next prompt",
        Some("composer presentation".into()),
        Vec::new(),
        Vec::new(),
    );
    save(prepared.clone());
    prepared.finish_active_turn(TurnStatus::Failed);
    prepared.status = SessionStatus::Failed;
    save(prepared);
    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap();
    assert_eq!(session.turns.last().unwrap().id, new_turn);
    assert_eq!(session.turns.last().unwrap().status, TurnStatus::Failed);
    assert!(
        session
            .messages
            .iter()
            .any(|message| message.display_content.as_deref() == Some("composer presentation"))
    );
    drop(state);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn delayed_startup_save_cannot_recreate_an_assigned_worktree() {
    let root = std::env::temp_dir().join(format!("task-view-workspace-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let (sink, _events) = EventSink::detached().tapped_connection();
    sink.set_task_view(false, 1, Default::default());
    assert!(sink.task_views_enabled());
    let mut draft = backend
        .task_store
        .load_session_detail(session_id)
        .unwrap()
        .unwrap();
    draft.workspace = SessionWorkspace::NewWorktree {
        base_branch: Some("dev".into()),
    };
    let save = |session| {
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id,
                    runtime_id: Uuid::nil(),
                    command: Command::SaveTaskState {
                        projects: Vec::new(),
                        live_session_ids: vec![session_id],
                        sessions: vec![session],
                        session_tails: Vec::new(),
                    },
                },
                sink.clone(),
                None,
            )
            .unwrap()
    };
    save(draft.clone());
    let mut assigned = draft.clone();
    assigned.workspace = SessionWorkspace::Worktree {
        path: root.join("assigned"),
        name: "assigned".into(),
        branch: None,
        base_branch: Some("dev".into()),
        adopted_by: None,
        borrowed: false,
    };
    save(assigned.clone());
    // A UI save captured while preparation was running arrives after registration.
    // Equal timestamps are realistic: submission and registration share a second.
    save(draft);
    let stored = backend
        .task_store
        .load_session_detail(session_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.workspace, assigned.workspace);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}
