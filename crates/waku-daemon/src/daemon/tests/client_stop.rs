use super::*;

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
        let mut forwarder = backend
            .register_runtime_forwarder(
                session_id,
                runtime_generation_entry(old_runtime, capture.clone(), &root),
                receiver,
                sink.clone(),
            )
            .unwrap();
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
        assert_eq!(*capture.cancels.lock(), 1);
        for client in &clients {
            assert!(matches!(next_event(client), DriverEvent::TurnCancelled));
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
            assert_eq!(*capture.cancels.lock(), 1);
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
