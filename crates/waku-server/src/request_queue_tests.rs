//! Behavioral checks for isolation and overload at the real dispatcher boundary.
use super::*;
use waku_protocol::model::ProviderKind;

struct SlowBackend {
    started: Sender<()>,
    release: Receiver<()>,
}

impl Backend for SlowBackend {
    fn handle(
        &self,
        request: Request,
        _: EventSink,
        _: Option<Uuid>,
    ) -> anyhow::Result<ResponsePayload> {
        if matches!(
            request.command,
            Command::ProbeFriend { .. }
                | Command::ProbeProvider { .. }
                | Command::SendFriendRequest { .. }
                | Command::SendFileToFriend { .. }
                | Command::SendMessageToFriend { .. }
                | Command::GetFriendSessions { .. }
                | Command::WatchFriendSession { .. }
                | Command::SandboxSignIn { .. }
        ) {
            self.started.send(())?;
            self.release.recv()?;
        }
        Ok(ResponsePayload::Ack)
    }
}

// Dropping every release sender wakes blocked workers even if a check panics.
fn fixture() -> (RequestDispatcher, Receiver<()>, Sender<()>) {
    let (started, started_rx) = unbounded();
    let (release, release_rx) = unbounded();
    let dispatcher = RequestDispatcher::new(
        Arc::new(SlowBackend {
            started,
            release: release_rx,
        }),
        Arc::new(Hub::default()),
    );
    (dispatcher, started_rx, release)
}

fn dispatch(
    dispatcher: &RequestDispatcher,
    command: Command,
    outgoing: &Sender<ServerMessage>,
) -> Uuid {
    let request_id = Uuid::new_v4();
    dispatcher.dispatch(
        Request {
            request_id,
            session_id: Uuid::new_v4(),
            runtime_id: Uuid::nil(),
            command,
        },
        outgoing.clone(),
        0,
        None,
    );
    request_id
}

fn percentile(samples: &mut [Duration], percentile: usize) -> Duration {
    samples.sort_unstable();
    samples[(samples.len() * percentile).div_ceil(100).saturating_sub(1)]
}

#[test]
fn slow_friends_and_sandbox_leave_controls_responsive() {
    let (dispatcher, started, release) = fixture();
    let (outgoing, replies) = unbounded();
    let commands = [
        Command::ProbeFriend {
            node_id: "controlled-peer".into(),
        },
        Command::SendFriendRequest {
            code: "controlled-peer".into(),
            name: "fixture".into(),
        },
        Command::SendFileToFriend {
            node_id: "controlled-peer".into(),
            path: "fixture".into(),
            title: None,
            note: None,
        },
        Command::SendMessageToFriend {
            node_id: "controlled-peer".into(),
            text: "fixture".into(),
        },
        Command::GetFriendSessions {
            node_id: "controlled-peer".into(),
            origin_url: "fixture".into(),
        },
        Command::WatchFriendSession {
            node_id: "controlled-peer".into(),
            origin_url: "fixture".into(),
            session_id: Uuid::new_v4(),
        },
        Command::SandboxSignIn {
            provider: ProviderKind::Codex,
        },
    ];
    let mut idle = Vec::new();
    for _ in 0..31 {
        let began = Instant::now();
        let id = dispatch(&dispatcher, Command::LoadTaskState, &outgoing);
        assert!(
            matches!(replies.recv_timeout(Duration::from_secs(1)).unwrap(), ServerMessage::Response { request_id, .. } if request_id == id)
        );
        idle.push(began.elapsed());
    }
    idle.remove(0);
    eprintln!(
        "queue-proof idle n=30 p95={:?} max={:?}",
        percentile(&mut idle, 95),
        idle.iter().max().unwrap()
    );
    let mut control_failures = Vec::new();
    let mut heavy_lane_failures = Vec::new();
    for command in commands {
        let label = pool_job_label(&command);
        let mut blocked = HashSet::new();
        for _ in 0..4 {
            blocked.insert(dispatch(&dispatcher, command.clone(), &outgoing));
        }
        for _ in 0..4 {
            started.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        // Long control waits must leave the existing heavy lane available.
        let heavy_probe_id = dispatch(
            &dispatcher,
            Command::ProbeProvider {
                provider: ProviderKind::Codex,
                binary_override: None,
                discover_models: false,
                probe_version: false,
            },
            &outgoing,
        );
        blocked.insert(heavy_probe_id);
        let heavy_probe_started_while_blocked =
            started.recv_timeout(Duration::from_millis(250)).is_ok();
        eprintln!(
            "queue-proof {label}: unrelated heavy ProbeProvider started before release={heavy_probe_started_while_blocked}"
        );
        // Check separately isolated lanes while long requests remain blocked.
        for command in [
            Command::GetDaemonStats,
            Command::HydrateSession {
                session_id: Uuid::new_v4(),
            },
        ] {
            let id = dispatch(&dispatcher, command, &outgoing);
            assert!(
                matches!(replies.recv_timeout(Duration::from_secs(1)).unwrap(), ServerMessage::Response { request_id, .. } if request_id == id)
            );
        }
        let mut pending = HashMap::new();
        for _ in 0..30 {
            let began = Instant::now();
            let id = dispatch(&dispatcher, Command::LoadTaskState, &outgoing);
            pending.insert(id, began);
        }
        let deadline = Instant::now() + Duration::from_millis(900);
        let mut samples = Vec::new();
        while !pending.is_empty() {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            let Ok(ServerMessage::Response {
                request_id,
                outcome,
            }) = replies.recv_timeout(remaining)
            else {
                break;
            };
            assert!(matches!(outcome, ResponseOutcome::Ok { .. }));
            samples.push(
                pending
                    .remove(&request_id)
                    .expect("only control probes complete before release")
                    .elapsed(),
            );
        }
        let responsive = pending.is_empty();
        eprintln!(
            "queue-proof {label}: {} / 30 controls completed before release",
            samples.len()
        );
        // Release before asserting so a baseline failure never strands work.
        for _ in 0..4 {
            release.send(()).unwrap();
        }
        release.send(()).unwrap();
        if !heavy_probe_started_while_blocked {
            started.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        while !blocked.is_empty() || !pending.is_empty() {
            let ServerMessage::Response {
                request_id,
                outcome,
            } = replies.recv_timeout(Duration::from_secs(2)).unwrap()
            else {
                panic!("expected response")
            };
            assert!(matches!(outcome, ResponseOutcome::Ok { .. }));
            if let Some(began) = pending.remove(&request_id) {
                samples.push(began.elapsed());
            } else {
                assert!(
                    blocked.remove(&request_id),
                    "every accepted request completes exactly once"
                );
            }
        }
        let p95 = percentile(&mut samples, 95);
        let maximum = *samples.iter().max().unwrap();
        eprintln!("queue-proof {label} n=30 p95={p95:?} max={maximum:?}");
        if !(responsive
            && p95 <= Duration::from_millis(250)
            && maximum < Duration::from_secs(1))
        {
            control_failures.push(label.clone());
        }
        if !heavy_probe_started_while_blocked {
            heavy_lane_failures.push(label);
        }
    }
    assert!(
        control_failures.is_empty(),
        "slow operations blocked unrelated controls: {control_failures:?}"
    );
    assert!(
        heavy_lane_failures.is_empty(),
        "slow operations blocked unrelated heavy work: {heavy_lane_failures:?}"
    );
}
