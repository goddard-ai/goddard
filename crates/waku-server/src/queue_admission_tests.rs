use super::*;
use waku_protocol::model::ProviderKind;

struct BlockedBackend {
    started: Sender<()>,
    release: Receiver<()>,
    executed: Arc<AtomicUsize>,
    executed_ids: Arc<Mutex<HashSet<Uuid>>>,
}
impl Backend for BlockedBackend {
    fn handle(
        &self,
        request: Request,
        _: EventSink,
        _: Option<Uuid>,
    ) -> anyhow::Result<ResponsePayload> {
        if matches!(
            &request.command,
            Command::ProbeProvider { .. } | Command::SendFriendRequest { .. }
        ) {
            self.executed.fetch_add(1, Ordering::Relaxed);
            self.executed_ids.lock().insert(request.request_id);
            self.started.send(())?;
            self.release.recv()?;
        }
        Ok(ResponsePayload::Ack)
    }
}
fn probe() -> Command {
    Command::ProbeProvider {
        provider: ProviderKind::Codex,
        binary_override: None,
        discover_models: false,
        probe_version: false,
    }
}
fn friend_request() -> Command {
    Command::SendFriendRequest {
        code: "controlled-peer".into(),
        name: "fixture".into(),
    }
}
fn request(command: Command) -> Request {
    Request {
        request_id: Uuid::new_v4(),
        session_id: Uuid::nil(),
        runtime_id: Uuid::nil(),
        command,
    }
}
fn connect(
    dispatcher: Arc<RequestDispatcher>,
    hub: Arc<Hub>,
) -> (
    tungstenite::WebSocket<TcpStream>,
    Arc<AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let thread = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = handle_connection(
            stream,
            "fixture",
            dispatcher,
            hub,
            server_shutdown.clone(),
            server_shutdown,
            &ServerOptions::default(),
        );
    });
    let stream = TcpStream::connect(address).unwrap();
    stream.set_nodelay(true).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    let (mut socket, _) = tungstenite::client(format!("ws://{address}/v1"), stream).unwrap();
    write_json(
        &mut socket,
        &ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            token: "fixture".into(),
            client_id: Uuid::new_v4(),
            resume_from: vec![],
        },
    )
    .unwrap();
    let message: ServerMessage =
        serde_json::from_str(socket.read().unwrap().into_text().unwrap().as_ref()).unwrap();
    assert!(matches!(message, ServerMessage::Hello { .. }));
    (socket, shutdown, thread)
}
fn read(socket: &mut tungstenite::WebSocket<TcpStream>) -> (Uuid, ResponseOutcome) {
    let message: ServerMessage =
        serde_json::from_str(socket.read().unwrap().into_text().unwrap().as_ref()).unwrap();
    match message {
        ServerMessage::Response {
            request_id,
            outcome,
        } => (request_id, outcome),
        other => panic!("unexpected {other:?}"),
    }
}
#[test]
fn saturated_admission_keeps_the_submitting_connection_responsive() {
    let (started, started_rx) = unbounded();
    let (release, release_rx) = unbounded();
    let executed = Arc::new(AtomicUsize::new(0));
    let executed_ids = Arc::new(Mutex::new(HashSet::new()));
    let hub = Arc::new(Hub::default());
    let dispatcher = Arc::new(RequestDispatcher::new(
        Arc::new(BlockedBackend {
            started,
            release: release_rx,
            executed: executed.clone(),
            executed_ids: executed_ids.clone(),
        }),
        hub.clone(),
    ));
    let (bulk_out, bulk_rx) = unbounded();
    for _ in 0..HEAVY_REQUEST_WORKERS {
        dispatcher.dispatch(request(probe()), bulk_out.clone(), 0, None);
    }
    for _ in 0..HEAVY_REQUEST_WORKERS {
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    for _ in 0..HEAVY_REQUEST_QUEUE {
        dispatcher.dispatch(request(probe()), bulk_out.clone(), 0, None);
    }
    assert_eq!(dispatcher.heavy_requests.jobs.len(), HEAVY_REQUEST_QUEUE);
    let saturation_started = Instant::now();
    let (mut same, same_stop, same_thread) = connect(dispatcher.clone(), hub.clone());
    let (mut other, other_stop, other_thread) = connect(dispatcher.clone(), hub);
    let mut pending = HashMap::new();
    for _ in 0..30 {
        let req = request(Command::GetDaemonStats);
        pending.insert(req.request_id, Instant::now());
        write_json(&mut same, &ClientMessage::Request(req)).unwrap();
    }
    let mut deferred_ids = HashSet::new();
    for _ in 0..POOL_DEFERRED_ADMISSIONS_PER_POOL {
        let req = request(probe());
        deferred_ids.insert(req.request_id);
        write_json(&mut same, &ClientMessage::Request(req)).unwrap();
    }
    let mut rejected_ids = HashSet::new();
    for _ in 0..HEAVY_REQUEST_QUEUE + POOL_DEFERRED_ADMISSIONS_PER_POOL {
        let rejected = request(probe());
        rejected_ids.insert(rejected.request_id);
        write_json(&mut same, &ClientMessage::Request(rejected)).unwrap();
    }
    let other_probes = std::thread::spawn(move || {
        let mut other_times = Vec::new();
        for _ in 0..30 {
            let req = request(Command::GetDaemonStats);
            let id = req.request_id;
            let began = Instant::now();
            write_json(&mut other, &ClientMessage::Request(req)).unwrap();
            let (got, outcome) = read(&mut other);
            assert_eq!(got, id);
            assert!(matches!(outcome, ResponseOutcome::Ok { .. }));
            other_times.push(began.elapsed());
        }
        (other, other_times)
    });
    let mut same_times = Vec::new();
    let mut remaining_rejected_ids = rejected_ids.clone();
    while !pending.is_empty() || !remaining_rejected_ids.is_empty() {
        let (id, outcome) = read(&mut same);
        if remaining_rejected_ids.remove(&id) {
            assert!(matches!(
                outcome,
                ResponseOutcome::Error { error }
                    if error.message.contains("busy")
                        && error.code == Some(waku_protocol::RpcErrorCode::AdmissionBusy)
            ));
        } else {
            assert!(
                pending.contains_key(&id),
                "unexpected pre-release response {id}"
            );
            assert!(matches!(outcome, ResponseOutcome::Ok { .. }));
            same_times.push(pending.remove(&id).unwrap().elapsed());
        }
    }
    let (mut other, mut other_times) = other_probes.join().unwrap();
    assert_eq!(executed.load(Ordering::Relaxed), HEAVY_REQUEST_WORKERS);
    assert!(rejected_ids.is_disjoint(&executed_ids.lock()));
    let held_for = saturation_started.elapsed();
    if held_for < Duration::from_millis(50) {
        std::thread::sleep(Duration::from_millis(50) - held_for);
    }
    assert!(saturation_started.elapsed() >= Duration::from_millis(50));
    let accepted_heavy = HEAVY_REQUEST_WORKERS + HEAVY_REQUEST_QUEUE + deferred_ids.len();
    for _ in 0..accepted_heavy {
        release.send(()).unwrap();
    }
    for _ in 0..HEAVY_REQUEST_WORKERS + HEAVY_REQUEST_QUEUE {
        assert!(matches!(
            bulk_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            ServerMessage::Response {
                outcome: ResponseOutcome::Ok { .. },
                ..
            }
        ));
    }
    let mut deferred_seen = HashSet::new();
    while deferred_seen.len() < deferred_ids.len() {
        let (id, outcome) = read(&mut same);
        assert!(
            deferred_ids.contains(&id),
            "unexpected deferred response {id}"
        );
        assert!(deferred_seen.insert(id), "duplicate deferred response {id}");
        assert!(matches!(outcome, ResponseOutcome::Ok { .. }));
    }
    assert_eq!(executed.load(Ordering::Relaxed), accepted_heavy);
    assert_eq!(executed_ids.lock().len(), accepted_heavy);
    assert_eq!(
        dispatcher
            .heavy_requests
            .deferred_count
            .load(Ordering::Acquire),
        0
    );
    assert_eq!(
        dispatcher
            .health_requests
            .deferred_count
            .load(Ordering::Acquire),
        0
    );
    same_times.sort();
    other_times.sort();
    eprintln!(
        "admission-proof held={:?} n=30 same p95={:?} max={:?}; second p95={:?} max={:?}; bounded_waiting={} rejected={}",
        saturation_started.elapsed(),
        same_times[28],
        same_times[29],
        other_times[28],
        other_times[29],
        HEAVY_REQUEST_QUEUE + deferred_ids.len(),
        rejected_ids.len()
    );
    let _ = same.close(None);
    let _ = other.close(None);
    same_stop.store(true, Ordering::Release);
    other_stop.store(true, Ordering::Release);
    same_thread.join().unwrap();
    other_thread.join().unwrap();
    assert!(
        same_times[28] <= Duration::from_millis(250) && same_times[29] < Duration::from_secs(1),
        "full pool stalled the socket loop"
    );
}

#[test]
fn deferred_admission_respects_count_and_encoded_byte_bounds() {
    let pool = IndependentRequestPool::new("deferred-bounds-test", 1, 1);
    let largest = pool.reserve_deferred(POOL_DEFERRED_BYTES_PER_POOL).unwrap();
    assert!(pool.reserve_deferred(1).is_none());
    drop(largest);

    let slots: Vec<_> = (0..POOL_DEFERRED_ADMISSIONS_PER_POOL)
        .map(|_| pool.reserve_deferred(0).unwrap())
        .collect();
    assert!(pool.reserve_deferred(0).is_none());
    assert_eq!(
        request_pool_snapshot()["deferred-bounds-test"].queued,
        POOL_DEFERRED_ADMISSIONS_PER_POOL as u32
    );
    drop(slots);
    assert_eq!(pool.deferred_count.load(Ordering::Acquire), 0);
    assert_eq!(pool.deferred_bytes.load(Ordering::Acquire), 0);
    assert_eq!(request_pool_snapshot()["deferred-bounds-test"].queued, 0);
}

#[test]
fn expired_deferred_admission_is_reported_and_never_runs() {
    let (started, started_rx) = unbounded();
    let (release, release_rx) = unbounded();
    let executed = Arc::new(AtomicUsize::new(0));
    let executed_ids = Arc::new(Mutex::new(HashSet::new()));
    let hub = Arc::new(Hub::default());
    let dispatcher = RequestDispatcher::new(
        Arc::new(BlockedBackend {
            started,
            release: release_rx,
            executed: executed.clone(),
            executed_ids: executed_ids.clone(),
        }),
        hub.clone(),
    );
    let (outgoing, bulk_rx) = unbounded();
    for _ in 0..HEAVY_REQUEST_WORKERS {
        dispatcher.dispatch(request(probe()), outgoing.clone(), 0, None);
    }
    for _ in 0..HEAVY_REQUEST_WORKERS {
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    for _ in 0..HEAVY_REQUEST_QUEUE {
        dispatcher.dispatch(request(probe()), outgoing.clone(), 0, None);
    }

    let expired = request(probe());
    let expired_id = expired.request_id;
    let mut connection = ConnectionDispatchState::default();
    assert!(dispatcher.dispatch_from_connection(
        expired,
        outgoing.clone(),
        0,
        None,
        &mut connection,
        256,
    ));
    assert_eq!(connection.pending.len(), 1);
    connection.pending.front_mut().unwrap().deadline = Instant::now();
    for _ in 0..MAX_PENDING_ADMISSION_ERRORS {
        connection.responses.push_back(ServerMessage::Response {
            request_id: Uuid::new_v4(),
            outcome: ResponseOutcome::Ok {
                payload: ResponsePayload::Ack,
            },
        });
    }
    assert!(
        connection.retry_pending(&dispatcher, &hub, true),
        "an error backlog closed the connection before its responses could drain"
    );
    assert_eq!(connection.pending.len(), 1);
    assert_eq!(connection.responses.len(), MAX_PENDING_ADMISSION_ERRORS);
    connection.responses.clear();
    assert!(connection.retry_pending(&dispatcher, &hub, true));

    assert!(connection.pending.is_empty());
    assert_eq!(
        dispatcher
            .heavy_requests
            .deferred_count
            .load(Ordering::Acquire),
        0
    );
    assert!(!executed_ids.lock().contains(&expired_id));
    assert!(matches!(
        connection.responses.pop_front(),
        Some(ServerMessage::Response {
            request_id,
            outcome: ResponseOutcome::Error { error },
        }) if request_id == expired_id
            && error.code == Some(waku_protocol::RpcErrorCode::AdmissionBusy)
    ));

    for _ in 0..HEAVY_REQUEST_WORKERS + HEAVY_REQUEST_QUEUE {
        release.send(()).unwrap();
    }
    for _ in 0..HEAVY_REQUEST_WORKERS + HEAVY_REQUEST_QUEUE {
        assert!(matches!(
            bulk_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            ServerMessage::Response {
                outcome: ResponseOutcome::Ok { .. },
                ..
            }
        ));
    }
    assert_eq!(
        executed.load(Ordering::Relaxed),
        HEAVY_REQUEST_WORKERS + HEAVY_REQUEST_QUEUE
    );
}

#[test]
fn slow_or_disconnected_connection_keeps_other_admission_independent() {
    let (started, started_rx) = unbounded();
    let (release, release_rx) = unbounded();
    let executed = Arc::new(AtomicUsize::new(0));
    let executed_ids = Arc::new(Mutex::new(HashSet::new()));
    let hub = Arc::new(Hub::default());
    let dispatcher = Arc::new(RequestDispatcher::new(
        Arc::new(BlockedBackend {
            started,
            release: release_rx,
            executed: executed.clone(),
            executed_ids: executed_ids.clone(),
        }),
        hub.clone(),
    ));
    let (bulk_out, _bulk_rx) = unbounded();
    let slow_mutation = friend_request();
    for _ in 0..LONG_CONTROL_REQUEST_WORKERS {
        dispatcher.dispatch(request(slow_mutation.clone()), bulk_out.clone(), 0, None);
    }
    for _ in 0..LONG_CONTROL_REQUEST_WORKERS {
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    for _ in 0..LONG_CONTROL_REQUEST_QUEUE {
        dispatcher.dispatch(request(slow_mutation.clone()), bulk_out.clone(), 0, None);
    }

    // An unread response channel must not trap deferred-admission errors in a
    // shared worker or prevent another connection from making progress.
    let (stalled_outgoing, stalled_rx) = bounded(1);
    stalled_outgoing
        .send(ServerMessage::Response {
            request_id: Uuid::new_v4(),
            outcome: ResponseOutcome::Ok {
                payload: ResponsePayload::Ack,
            },
        })
        .unwrap();
    let expired = request(slow_mutation.clone());
    let expired_id = expired.request_id;
    let mut stalled = ConnectionDispatchState::default();
    assert!(dispatcher.dispatch_from_connection(
        expired,
        stalled_outgoing,
        0,
        None,
        &mut stalled,
        256,
    ));
    stalled.pending.front_mut().unwrap().deadline = Instant::now();
    let (retried, retried_rx) = bounded(1);
    let retry_dispatcher = dispatcher.clone();
    let retry_hub = hub.clone();
    let retry_thread = std::thread::spawn(move || {
        let began = Instant::now();
        let continued = stalled.retry_pending(&retry_dispatcher, &retry_hub, true);
        assert!(stalled.pending.is_empty());
        retried
            .send((continued, stalled.responses.pop_front(), began.elapsed()))
            .unwrap();
    });
    let (continued, response, stalled_retry_elapsed) =
        match retried_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(result) => result,
            Err(error) => {
                drop(stalled_rx);
                retry_thread.join().unwrap();
                panic!("unread output blocked admission rejection: {error}");
            }
        };
    retry_thread.join().unwrap();
    assert!(continued);
    assert_eq!(stalled_rx.len(), 1);
    assert!(matches!(
        response,
        Some(ServerMessage::Response {
            request_id,
            outcome: ResponseOutcome::Error { error },
        }) if request_id == expired_id
            && error.code == Some(waku_protocol::RpcErrorCode::AdmissionBusy)
    ));

    let (mut disconnected, disconnected_stop, disconnected_thread) =
        connect(dispatcher.clone(), hub.clone());
    let rejected_before = request_pool_snapshot()["long-control"].rejected;
    let saturation_started = Instant::now();
    let mut pending_ids = HashSet::new();
    for _ in 0..POOL_DEFERRED_ADMISSIONS_PER_POOL {
        let pending = request(slow_mutation.clone());
        pending_ids.insert(pending.request_id);
        write_json(&mut disconnected, &ClientMessage::Request(pending)).unwrap();
    }
    let rejected = request(slow_mutation.clone());
    let rejected_id = rejected.request_id;
    write_json(&mut disconnected, &ClientMessage::Request(rejected)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while dispatcher
        .long_control_requests
        .deferred_count
        .load(Ordering::Acquire)
        != POOL_DEFERRED_ADMISSIONS_PER_POOL
        || request_pool_snapshot()["long-control"].rejected != rejected_before + 1
    {
        assert!(
            Instant::now() < deadline,
            "deferred or overflow request was not processed"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let (got, outcome) = read(&mut disconnected);
    assert_eq!(got, rejected_id);
    assert!(matches!(
        outcome,
        ResponseOutcome::Error { error }
            if error.message.contains("busy")
                && error.code == Some(waku_protocol::RpcErrorCode::AdmissionBusy)
    ));

    let (mut other, other_stop, other_thread) = connect(dispatcher.clone(), hub.clone());
    let unrelated = request(Command::GetDaemonStats);
    let unrelated_id = unrelated.request_id;
    let began = Instant::now();
    write_json(&mut other, &ClientMessage::Request(unrelated)).unwrap();
    let (got, outcome) = read(&mut other);
    assert_eq!(got, unrelated_id);
    assert!(matches!(outcome, ResponseOutcome::Ok { .. }));
    let unrelated_elapsed = began.elapsed();
    assert!(unrelated_elapsed <= Duration::from_millis(250));
    assert_eq!(stalled_rx.len(), 1);
    drop(stalled_rx);

    let _ = disconnected.close(None);
    disconnected_stop.store(true, Ordering::Release);
    if saturation_started.elapsed() < Duration::from_millis(50) {
        std::thread::sleep(Duration::from_millis(50) - saturation_started.elapsed());
    }
    assert!(saturation_started.elapsed() >= Duration::from_millis(50));
    let accepted = LONG_CONTROL_REQUEST_WORKERS + LONG_CONTROL_REQUEST_QUEUE + pending_ids.len();
    for _ in 0..accepted {
        release.send(()).unwrap();
    }
    for _ in 0..LONG_CONTROL_REQUEST_QUEUE + pending_ids.len() {
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    disconnected_thread.join().unwrap();
    assert_eq!(
        dispatcher
            .long_control_requests
            .deferred_count
            .load(Ordering::Acquire),
        0
    );
    assert_eq!(executed.load(Ordering::Relaxed), accepted);
    let executed_ids = executed_ids.lock();
    assert_eq!(executed_ids.len(), accepted);
    assert!(pending_ids.iter().all(|id| executed_ids.contains(id)));
    assert!(!executed_ids.contains(&rejected_id));
    assert!(!executed_ids.contains(&expired_id));
    drop(executed_ids);

    let _ = other.close(None);
    other_stop.store(true, Ordering::Release);
    other_thread.join().unwrap();
    eprintln!(
        "admission-disconnect held={:?} accepted={} rejected=1 unread-output rejection={stalled_retry_elapsed:?} unrelated control={unrelated_elapsed:?}",
        saturation_started.elapsed(),
        accepted
    );
}
