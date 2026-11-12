//! Stop delivery and the ordering cut it makes in an occupied runtime mailbox.
use super::*;

struct HeldRuntime {
    runtime_id: Uuid,
    started: Sender<()>,
    release: Receiver<()>,
    stopped: Sender<Uuid>,
    signals: Sender<()>,
    prompts: Sender<String>,
}

impl HeldRuntime {
    fn run(
        &self,
        request: Request,
        cancellation: &RuntimeCancellation,
    ) -> anyhow::Result<ResponsePayload> {
        if matches!(
            &request.command,
            Command::Rollback { .. } | Command::Prompt { .. } | Command::Cancel
        ) {
            anyhow::ensure!(request.runtime_id == self.runtime_id, "stale runtime");
        }
        if matches!(
            &request.command,
            Command::Rollback { .. } | Command::Prompt { .. }
        ) {
            let signals = self.signals.clone();
            cancellation.bind_runtime(request.runtime_id, move || {
                let _ = signals.send(());
            });
        }
        match request.command {
            Command::Rollback { .. } => {
                self.started.send(())?;
                self.release.recv()?;
            }
            Command::Cancel => {
                self.signals.send(())?;
                self.stopped.send(request.request_id)?;
            }
            Command::Prompt { prompt, .. } => {
                if prompt == "preparing prompt" {
                    self.started.send(())?;
                    self.release.recv()?;
                }
                cancellation.deliver(|| {
                    self.prompts.send(prompt)?;
                    Ok(())
                })?;
            }
            _ => {}
        }
        Ok(ResponsePayload::Ack)
    }
}

impl Backend for HeldRuntime {
    fn handle(
        &self,
        request: Request,
        _: EventSink,
        _: Option<Uuid>,
    ) -> anyhow::Result<ResponsePayload> {
        self.run(request, &RuntimeCancellation::default())
    }

    fn handle_with_cancellation(
        &self,
        request: Request,
        _: EventSink,
        _: Option<Uuid>,
        cancellation: &RuntimeCancellation,
    ) -> anyhow::Result<ResponsePayload> {
        self.run(request, cancellation)
    }
}

struct Fixture {
    dispatcher: RequestDispatcher,
    started: Receiver<()>,
    release: Sender<()>,
    stopped: Receiver<Uuid>,
    signals: Receiver<()>,
    prompts: Receiver<String>,
    outgoing: Sender<ServerMessage>,
    replies: Receiver<ServerMessage>,
    session: Uuid,
    runtime: Uuid,
}

impl Fixture {
    fn new() -> Self {
        let (started, started_rx) = unbounded();
        let (release, release_rx) = unbounded();
        let (stopped, stopped_rx) = unbounded();
        let (signals, signals_rx) = unbounded();
        let (prompts, prompt_rx) = unbounded();
        let (outgoing, replies) = unbounded();
        let runtime = Uuid::new_v4();
        Self {
            dispatcher: RequestDispatcher::new(
                Arc::new(HeldRuntime {
                    runtime_id: runtime,
                    started,
                    release: release_rx,
                    stopped,
                    signals,
                    prompts,
                }),
                Arc::new(Hub::default()),
            ),
            started: started_rx,
            release,
            stopped: stopped_rx,
            signals: signals_rx,
            prompts: prompt_rx,
            outgoing,
            replies,
            session: Uuid::new_v4(),
            runtime,
        }
    }

    fn send(&self, command: Command) -> Uuid {
        let request_id = Uuid::new_v4();
        self.send_as(command, self.runtime, request_id, None);
        request_id
    }

    fn send_as(&self, command: Command, runtime_id: Uuid, request_id: Uuid, agent: Option<Uuid>) {
        self.dispatcher.dispatch(
            Request {
                request_id,
                session_id: self.session,
                runtime_id,
                command,
            },
            self.outgoing.clone(),
            0,
            agent,
        );
    }

    fn replies(&self, count: usize) -> HashMap<Uuid, ResponseOutcome> {
        (0..count)
            .map(
                |_| match self.replies.recv_timeout(Duration::from_secs(5)).unwrap() {
                    ServerMessage::Response {
                        request_id,
                        outcome,
                    } => (request_id, outcome),
                    _ => panic!("expected a correlated reply"),
                },
            )
            .collect()
    }
}

fn prompt(text: &str) -> Command {
    Command::Prompt {
        prompt: text.into(),
        runtime_mode: None,
        turn_id: None,
        message_id: None,
        hidden: false,
        attachments: Vec::new(),
    }
}

#[test]
fn stop_signals_the_running_operation_without_waiting_for_its_mailbox() {
    let fixture = Fixture::new();
    fixture.send(Command::Rollback { turns: 1 });
    fixture
        .started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let began = Instant::now();
    let expected: HashSet<_> = (0..30).map(|_| fixture.send(Command::Cancel)).collect();
    let deadline = began + Duration::from_millis(250);
    let mut signals = Vec::new();
    while signals.len() < 30 {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        let Ok(()) = fixture.signals.recv_timeout(remaining) else {
            break;
        };
        signals.push(began.elapsed());
    }
    let delivered_before_release = signals.len();
    fixture.release.send(()).unwrap();
    let replies = fixture.replies(31);
    assert_eq!(fixture.stopped.try_iter().collect::<HashSet<_>>(), expected);
    assert_eq!(replies.len(), 31, "each accepted request replies once");
    assert!(
        replies
            .values()
            .all(|outcome| matches!(outcome, ResponseOutcome::Ok { .. }))
    );
    assert_eq!(
        delivered_before_release, 30,
        "Stop waited behind the operation it should interrupt"
    );
}

#[test]
fn stop_prevents_an_active_prompt_preparation_from_starting_a_turn_after_stop() {
    let fixture = Fixture::new();
    let active = fixture.send(prompt("preparing prompt"));
    fixture
        .started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    fixture.send(Command::Cancel);
    let later = fixture.send(prompt("sent after Stop"));
    fixture.release.send(()).unwrap();
    let replies = fixture.replies(3);
    let executed: Vec<_> = fixture.prompts.try_iter().collect();
    assert!(
        matches!(replies[&active], ResponseOutcome::Error { .. }),
        "a stopped active preparation must not start a turn after Stop"
    );
    assert!(matches!(replies[&later], ResponseOutcome::Ok { .. }));
    assert_eq!(executed, ["sent after Stop"]);
}

#[test]
fn scoped_and_stale_stop_requests_cannot_signal_the_bound_driver() {
    for scoped in [false, true] {
        let fixture = Fixture::new();
        fixture.send(Command::Rollback { turns: 1 });
        fixture
            .started
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let id = Uuid::new_v4();
        fixture.send_as(
            Command::Cancel,
            if scoped {
                fixture.runtime
            } else {
                Uuid::new_v4()
            },
            id,
            scoped.then(Uuid::new_v4),
        );
        let signaled = fixture.signals.try_recv().is_ok();
        fixture.release.send(()).unwrap();
        let replies = fixture.replies(2);
        assert!(
            !signaled,
            "an unauthorized or stale Stop reached the driver"
        );
        assert!(matches!(replies[&id], ResponseOutcome::Error { .. }));
        assert!(fixture.signals.is_empty());
        assert!(fixture.stopped.is_empty());
    }
}

#[test]
fn replaying_a_completed_stop_does_not_interrupt_a_later_operation() {
    let fixture = Fixture::new();
    let original = fixture.send(Command::Cancel);
    fixture.replies(1);
    assert_eq!(
        fixture
            .stopped
            .recv_timeout(Duration::from_secs(1))
            .unwrap(),
        original
    );
    fixture.signals.try_iter().for_each(drop);
    fixture.send(Command::Rollback { turns: 1 });
    fixture
        .started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    fixture.send_as(Command::Cancel, fixture.runtime, original, None);
    let signaled = fixture.signals.try_recv().is_ok();
    fixture.release.send(()).unwrap();
    let replies = fixture.replies(2);
    assert!(!signaled);
    assert!(matches!(replies[&original], ResponseOutcome::Ok { .. }));
    assert!(fixture.signals.is_empty());
    assert!(fixture.stopped.is_empty(), "cached Stop executed again");
}

#[test]
fn stop_preserves_queued_user_prompt_order() {
    let fixture = Fixture::new();
    fixture.send(Command::Rollback { turns: 1 });
    fixture
        .started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    fixture.send(prompt("earlier accepted prompt"));
    fixture.send(Command::Cancel);
    fixture.send(prompt("later prompt"));
    fixture.release.send(()).unwrap();
    let replies = fixture.replies(4);
    assert!(
        replies
            .values()
            .all(|reply| matches!(reply, ResponseOutcome::Ok { .. }))
    );
    assert_eq!(
        fixture.prompts.try_iter().collect::<Vec<_>>(),
        ["earlier accepted prompt", "later prompt"]
    );
}

#[test]
fn stop_and_final_prompt_enqueue_have_one_order() {
    let cancellation = RuntimeCancellation::default();
    let runtime = Uuid::new_v4();
    let (wire, received) = unbounded();
    let interrupt = wire.clone();
    cancellation.bind_runtime(runtime, move || {
        interrupt.send("stop").unwrap();
    });
    let (started, starting) = unbounded();
    std::thread::scope(|scope| {
        cancellation
            .deliver(|| {
                scope.spawn(|| {
                    started.send(()).unwrap();
                    cancellation.signal(runtime);
                });
                starting.recv_timeout(Duration::from_secs(5)).unwrap();
                // Stop has started, but the final enqueue owns the gate.
                wire.send("prompt")?;
                Ok(())
            })
            .unwrap();
    });
    assert_eq!(received.try_iter().collect::<Vec<_>>(), ["prompt", "stop"]);
    assert!(
        cancellation
            .deliver(|| {
                wire.send("late prompt")?;
                Ok(())
            })
            .is_err()
    );
    assert!(received.try_recv().is_err());
}
