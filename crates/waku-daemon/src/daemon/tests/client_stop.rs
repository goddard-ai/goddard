use super::*;

#[test]
fn cancelled_prompt_preparation_is_acknowledged_and_stays_user_interrupted() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    struct Preparing {
        capture: Arc<CaptureDriver>,
        started: crossbeam_channel::Sender<()>,
        release: crossbeam_channel::Receiver<()>,
        cancelled: crossbeam_channel::Sender<()>,
    }
    impl driver::DriverControl for Preparing {
        fn prompt(&self, prompt: String) {
            self.capture.prompt(prompt);
        }
        fn prompt_with_gate(
            &self,
            prompt: String,
            attachments: Vec<crate::model::MessageAttachment>,
            gate: &driver::EnqueueGate<'_>,
        ) -> anyhow::Result<()> {
            self.started.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(5)).unwrap();
            self.capture.prompt_with_gate(prompt, attachments, gate)
        }
        fn cancel(&self) {
            let _ = self.cancelled.send(());
        }
        fn respond(&self, _: String, _: String) {}
        fn rollback(&self, _: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
            Ok(None)
        }
    }
    struct ServerFixture {
        root: PathBuf,
        shutdown: Arc<AtomicBool>,
        release: crossbeam_channel::Sender<()>,
        server: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for ServerFixture {
        fn drop(&mut self) {
            let _ = self.release.try_send(());
            self.shutdown.store(true, Ordering::Release);
            if let Some(server) = self.server.take() {
                let _ = server.join();
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    let root = std::env::temp_dir().join(format!("stop-prepared-prompt-{}", Uuid::new_v4()));
    let (backend, session, _) = read_scope_test_backend(&root);
    let backend = Arc::new(backend);
    let runtime = Uuid::new_v4();
    let capture = Arc::new(CaptureDriver::default());
    let (started, preparation) = crossbeam_channel::bounded(1);
    let (release, released) = crossbeam_channel::bounded(1);
    let (cancelled, signal) = crossbeam_channel::unbounded();
    let mut entry = runtime_generation_entry(runtime, capture.clone(), &root);
    entry.driver = DriverHandle::from_control(Arc::new(Preparing {
        capture: capture.clone(),
        started,
        release: released,
        cancelled,
    }));
    backend.sessions.lock().insert(session, entry);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server_backend = backend.clone();
    let server = std::thread::spawn(move || {
        crate::serve(
            listener,
            "local-fixture".into(),
            server_backend,
            server_shutdown,
            crate::server::ServerOptions::default(),
        )
        .unwrap();
    });
    let fixture = ServerFixture {
        root,
        shutdown,
        release,
        server: Some(server),
    };
    let sender = Arc::new(
        waku_client::DaemonClient::connect(&address.to_string(), "local-fixture".into()).unwrap(),
    );
    let stopper = Arc::new(
        waku_client::DaemonClient::connect(&address.to_string(), "local-fixture".into()).unwrap(),
    );
    backend
        .event_source
        .lock()
        .clone()
        .begin_session_runtime(session, runtime);
    let clients = [
        sender.subscribe(session, runtime),
        stopper.subscribe(session, runtime),
    ];
    let mut signals = Vec::new();
    let mut received = Vec::new();
    let mut outcomes = Vec::new();
    for probe in 0..30 {
        // The ordered Cancel also signals the fake driver. Never mistake a
        // prior iteration's terminal cleanup for this request's early signal.
        signal.try_iter().for_each(drop);
        let prompt_client = sender.clone();
        let prompt = std::thread::spawn(move || {
            prompt_client.request_with_timeout(
                session,
                runtime,
                Command::Prompt {
                    prompt: "prepared, then stopped".into(),
                    runtime_mode: None,
                    turn_id: Some(Uuid::new_v4()),
                    message_id: Some(Uuid::new_v4()),
                    hidden: false,
                    attachments: Vec::new(),
                },
                Some(Duration::from_secs(5)),
            )
        });
        preparation.recv_timeout(Duration::from_secs(5)).unwrap();
        let stop_client = if probe % 2 == 0 {
            sender.clone()
        } else {
            stopper.clone()
        };
        let began = std::time::Instant::now();
        let stop = std::thread::spawn(move || {
            stop_client.request_with_timeout(
                session,
                runtime,
                Command::Cancel,
                Some(Duration::from_secs(5)),
            )
        });
        let reached_before_release = signal.recv_timeout(Duration::from_secs(1)).is_ok();
        signals.push(began.elapsed());
        fixture.release.send(()).unwrap();
        let prompt_result = prompt.join().unwrap();
        let stop_result = stop.join().unwrap();
        for events in &clients {
            let mut kinds = Vec::new();
            loop {
                let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
                kinds.push(event.event.kind.clone());
                if event.event.kind == "turnCancelled" {
                    break;
                }
            }
            received.push(kinds);
        }
        let turn = backend
            .task_state
            .lock()
            .session_mut(session)
            .unwrap()
            .turns
            .last()
            .unwrap()
            .clone();
        outcomes.push((reached_before_release, prompt_result, stop_result, turn));
    }
    sender.disconnect(Duration::from_secs(1));
    stopper.disconnect(Duration::from_secs(1));
    drop(fixture);
    signals.sort_unstable();
    eprintln!(
        "cancelled prompt preparation:30 signals p95={:?} max={:?}",
        signals[28], signals[29]
    );
    assert!(signals[28] <= Duration::from_millis(250));
    assert!(signals[29] < Duration::from_secs(1));
    for (reached_before_release, prompt_result, stop_result, turn) in outcomes {
        assert!(reached_before_release, "Stop waited for prompt preparation");
        assert!(
            matches!(prompt_result, Ok(ResponsePayload::Ack)),
            "expected a cancelled admission, not a failed prompt: {prompt_result:?}"
        );
        assert!(matches!(stop_result, Ok(ResponsePayload::Ack)));
        assert_eq!(turn.status, TurnStatus::Interrupted);
        assert_eq!(turn.interruption, Some(crate::model::TurnInterruption::You));
    }
    assert!(capture.prompts.lock().is_empty());
    for events in received {
        assert!(events.iter().any(|kind| kind == "promptSubmitted"));
        assert!(
            !events
                .iter()
                .any(|kind| matches!(kind.as_str(), "error" | "localizedError" | "turnStarted"))
        );
    }
}

fn next_event(events: &crossbeam_channel::Receiver<waku_protocol::ServerMessage>) -> DriverEvent {
    loop {
        if let waku_protocol::ServerMessage::Event(event) = events
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("client must receive the runtime event")
        {
            return waku_protocol::event_from_wire(event.event).unwrap();
        }
    }
}

#[test]
fn clients_follow_stop_and_the_next_turn_on_retained_and_replaced_runtimes() {
    for early_signal in [false, true] {
        check_stop_and_next_turn(early_signal);
    }
}

fn check_stop_and_next_turn(early_signal: bool) {
    for replace in [false, true] {
        let root = std::env::temp_dir().join(format!("client-stop-{}", Uuid::new_v4()));
        let (backend, session_id, _) = read_scope_test_backend(&root);
        {
            let mut state = backend.task_state.lock();
            let session = state.session_mut(session_id).unwrap();
            backend.task_store.hydrate(session).unwrap();
            session.begin_turn("first prompt");
            session.mark_active_turn_provider_started();
            state.mark_session_dirty(session_id);
            backend.task_store.save(&mut state).unwrap();
        }
        let old_runtime = Uuid::new_v4();
        let sink = EventSink::detached().begin_session_runtime(session_id, old_runtime);
        let clients = [sink.tapped_events(), sink.tapped_events()];
        let capture = Arc::new(CaptureDriver::default());
        let (mut sender, receiver) = driver::test_event_channel();
        let entry = runtime_generation_entry(old_runtime, capture.clone(), &root);
        let immediate = entry.driver.clone();
        let mut forwarder = backend
            .register_runtime_forwarder(session_id, entry, receiver, sink.clone())
            .unwrap();
        if early_signal {
            immediate.cancel_from_user();
            sender
                .send(DriverEvent::TurnFinished {
                    success: false,
                    summary: Some("provider acknowledged interrupt".into()),
                    summary_i18n: None,
                })
                .unwrap();
            for client in &clients {
                assert!(matches!(next_event(client), DriverEvent::TurnCancelled));
            }
            let state = backend.task_state.lock();
            let turn = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .unwrap()
                .turns
                .last()
                .unwrap();
            assert_eq!(turn.status, TurnStatus::Interrupted);
            assert_eq!(turn.interruption, Some(crate::model::TurnInterruption::You));
        }
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id,
                    runtime_id: old_runtime,
                    command: Command::Cancel,
                },
                sink.clone(),
                None,
            )
            .unwrap();
        assert_eq!(*capture.cancels.lock(), if early_signal { 2 } else { 1 });
        for client in &clients {
            loop {
                match next_event(client) {
                    DriverEvent::TurnCancelled => break,
                    DriverEvent::TurnFinished { success: false, .. } if early_signal => {}
                    event => panic!("unexpected event before ordered Stop: {event:?}"),
                }
            }
        }
        // A late provider start/output/settle cannot rewrite the durable
        // user interruption or append output to a turn with no owner.
        for event in [
            DriverEvent::TurnStarted,
            DriverEvent::TextDelta("late output".into()),
            DriverEvent::TurnFinished {
                success: false,
                summary: None,
                summary_i18n: None,
            },
        ] {
            record_boss_event(&backend.task_state, &backend.task_store, session_id, &event)
                .unwrap();
        }
        {
            let state = backend.task_state.lock();
            let session = state.sessions.iter().find(|s| s.id == session_id).unwrap();
            assert_eq!(session.status, SessionStatus::Idle);
            let turn = session.turns.last().unwrap();
            assert_eq!(turn.status, TurnStatus::Interrupted);
            assert_eq!(turn.interruption, Some(crate::model::TurnInterruption::You));
            assert!(session.messages.iter().any(|message| matches!(
                message.notice,
                Some(crate::model::TranscriptNotice::Status {
                    kind: crate::model::TranscriptNoticeStatus::Stopped,
                })
            )));
            assert!(!session.messages.iter().any(|m| m.content == "late output"));
        }

        let runtime_id = if replace {
            backend
                .handle(
                    Request {
                        request_id: Uuid::new_v4(),
                        session_id,
                        runtime_id: old_runtime,
                        command: Command::CloseSession,
                    },
                    sink.clone(),
                    None,
                )
                .unwrap();
            for client in &clients {
                assert!(matches!(next_event(client), DriverEvent::ProcessExited));
            }
            drop(sender);
            forwarder.join().unwrap();
            let new_runtime = Uuid::new_v4();
            let new_sink = sink.begin_session_runtime(session_id, new_runtime);
            let (new_sender, receiver) = driver::test_event_channel();
            sender = new_sender;
            forwarder = backend
                .register_runtime_forwarder(
                    session_id,
                    runtime_generation_entry(new_runtime, capture.clone(), &root),
                    receiver,
                    new_sink,
                )
                .unwrap();
            // An old client's delayed Stop has no authority over the new turn.
            assert!(
                backend
                    .handle(
                        Request {
                            request_id: Uuid::new_v4(),
                            session_id,
                            runtime_id: old_runtime,
                            command: Command::Cancel,
                        },
                        sink.clone(),
                        None,
                    )
                    .is_err()
            );
            assert_eq!(*capture.cancels.lock(), if early_signal { 2 } else { 1 });
            new_runtime
        } else {
            old_runtime
        };
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id,
                    runtime_id,
                    command: Command::Prompt {
                        runtime_mode: None,
                        prompt: "next prompt".into(),
                        turn_id: Some(Uuid::new_v4()),
                        message_id: Some(Uuid::new_v4()),
                        hidden: false,
                        attachments: Vec::new(),
                    },
                },
                sink.for_session(session_id, runtime_id),
                None,
            )
            .unwrap();
        sender.send(DriverEvent::TurnStarted).unwrap();
        sender
            .send(DriverEvent::TextDelta("next answer".into()))
            .unwrap();
        for client in &clients {
            assert!(
                matches!(next_event(client), DriverEvent::PromptSubmitted { message, .. } if message == "next prompt")
            );
            assert!(matches!(next_event(client), DriverEvent::TurnStarted));
            assert!(
                matches!(next_event(client), DriverEvent::TextDelta(text) if text == "next answer")
            );
        }
        drop(sender);
        forwarder.join().unwrap();
        backend.shutdown();
        drop(backend);
        std::fs::remove_dir_all(root).ok();
    }
}
