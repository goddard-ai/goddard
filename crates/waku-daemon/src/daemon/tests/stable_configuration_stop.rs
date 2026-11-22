use super::*;
use crate::stable_runtime::confirmation_test_fixture::ConfirmationFixture;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use waku_runtime::contract::{CanonicalAction, ProviderEvent};

struct ServerFixture {
    confirmation: Arc<ConfirmationFixture>,
    shutdown: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
    root: PathBuf,
}
impl Drop for ServerFixture {
    fn drop(&mut self) {
        self.confirmation.stop();
        self.shutdown.store(true, Ordering::Release);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Observation {
    early: bool,
    stop_elapsed: Duration,
    cancelled_before_activation: bool,
    prompt_executed: bool,
    update_applied: Option<bool>,
    interrupted: bool,
}

#[test]
fn configured_stable_defaults_do_not_reuse_an_explicit_selection() {
    let mut refused = Vec::new();
    for clear_model in [true, false] {
        let root = std::env::temp_dir().join(format!("stable-defaults-{}", Uuid::new_v4()));
        let rejected = {
            let (backend, task) = surface_test_backend(&root);
            let backend = Arc::new(backend);
            backend.install_weak_self();
            let runtime = Uuid::new_v4();
            let fixture = ConfirmationFixture::new(&backend, task, runtime);
            let adapter = fixture.factory.for_session(runtime).unwrap();
            let explicit = crate::driver::SessionOptions {
                mode: RuntimeMode::Ask,
                model: Some("initial-model".into()),
                reasoning_effort: Some("high".into()),
                service_tier: None,
                context_window: None,
            };
            assert!(fixture.driver.apply_options(explicit.clone()));
            adapter.emit(ProviderEvent::ConfigActivated {
                revision: 1,
                boundary: waku_runtime::contract::ActivationBoundary::Immediate,
                effective: waku_runtime::contract::RequestedConfig {
                    provider: Some("claude".into()),
                    model: explicit.model.clone(),
                    effort: explicit.reasoning_effort.clone(),
                    options: serde_json::Value::Null,
                },
            });
            let mut defaults = explicit.clone();
            if clear_model {
                defaults.model = None;
            } else {
                defaults.reasoning_effort = None;
            }
            // A default selection must be reset or explicitly refused, never
            // confirmed merely because its absent field matches anything.
            let rejected = fixture
                .driver
                .prepare_prompt_options(defaults)
                .is_err_and(|error| {
                    error
                        .downcast_ref::<waku_protocol::RpcError>()
                        .is_some_and(|rpc| {
                            rpc.code == Some(waku_protocol::RpcErrorCode::ConfigurationRejected)
                        })
                });
            assert!(fixture.driver.prepare_prompt_options(explicit).is_ok());
            rejected
        };
        let _ = std::fs::remove_dir_all(&root);
        refused.push(rejected);
    }
    assert_eq!(
        refused,
        [true, true],
        "defaults silently retained explicit settings"
    );
}

fn probe(configured_prompt: bool, same_connection: bool) -> Observation {
    let root = std::env::temp_dir().join(format!("stable-stop-{}", Uuid::new_v4()));
    let (mut backend, session) = surface_test_backend(&root);
    {
        let mut state = backend.task_state.lock();
        state.sessions[0].provider = ProviderKind::Claude;
        state.sessions[0].runtime_mode = RuntimeMode::Ask;
        state.sessions[0].model = Some("initial-model".into());
    }
    runtime_generation_configure_launch(&mut backend, &root);
    let backend = Arc::new(backend);
    backend.install_weak_self();
    let runtime = Uuid::new_v4();
    let confirmation = Arc::new(ConfirmationFixture::new(&backend, session, runtime));
    let mut entry = runtime_generation_entry(runtime, Arc::new(CaptureDriver::default()), &root);
    entry.provider = ProviderKind::Claude;
    entry.mode = RuntimeMode::Ask;
    entry.driver = confirmation.driver.clone();
    entry.stable = Some(confirmation.link.clone());
    backend.sessions.lock().insert(session, entry);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = shutdown.clone();
    let server_backend = backend.clone();
    let server = std::thread::spawn(move || {
        crate::serve(
            listener,
            "stable-control-fixture".into(),
            server_backend,
            server_shutdown,
            crate::server::ServerOptions::default(),
        )
        .unwrap();
    });
    let fixture = ServerFixture {
        confirmation: confirmation.clone(),
        shutdown,
        server: Some(server),
        root,
    };
    let sender = Arc::new(
        waku_client::DaemonClient::connect(&address.to_string(), "stable-control-fixture".into())
            .unwrap(),
    );
    let stopper = if same_connection {
        sender.clone()
    } else {
        Arc::new(
            waku_client::DaemonClient::connect(
                &address.to_string(),
                "stable-control-fixture".into(),
            )
            .unwrap(),
        )
    };
    backend
        .event_source
        .lock()
        .clone()
        .begin_session_runtime(session, runtime);
    let adapter = confirmation.factory.for_session(runtime).unwrap();
    if !configured_prompt {
        sender
            .request_with_timeout(
                session,
                runtime,
                Command::Prompt {
                    prompt: "pre-existing work".into(),
                    runtime_mode: None,
                    turn_id: Some(Uuid::new_v4()),
                    message_id: Some(Uuid::new_v4()),
                    hidden: false,
                    attachments: Vec::new(),
                },
                Some(Duration::from_secs(5)),
            )
            .unwrap();
        let (turn_id, delivery_id) = adapter
            .dispatched()
            .into_iter()
            .find_map(|action| match action {
                CanonicalAction::Submit {
                    turn_id,
                    delivery_id,
                    ..
                } => Some((turn_id, delivery_id)),
                _ => None,
            })
            .unwrap();
        adapter.emit(ProviderEvent::TurnStarted {
            turn_id,
            delivery_id,
        });
    }
    adapter.script_dispatch(waku_runtime::adapter::DispatchOutcome::Unknown {
        detail: "configuration awaits provider activation".into(),
    });
    let command = if configured_prompt {
        Command::PromptWithConfiguration {
            configuration: waku_protocol::PromptConfiguration {
                provider: ProviderKind::Claude,
                options: crate::driver::SessionOptions {
                    mode: RuntimeMode::Ask,
                    model: Some("selected-model".into()),
                    reasoning_effort: None,
                    service_tier: None,
                    context_window: None,
                },
            },
            prompt: "stop before executing this prompt".into(),
            turn_id: Some(Uuid::new_v4()),
            message_id: Some(Uuid::new_v4()),
            hidden: false,
            attachments: Vec::new(),
            presentation: None,
        }
    } else {
        Command::ApplyOptions {
            options: waku_protocol::WireSessionOptions {
                mode: "ask".into(),
                model: Some("selected-model".into()),
                reasoning_effort: None,
                service_tier: None,
                context_window: None,
            },
        }
    };
    let (early, stop_elapsed, cancelled_before_activation, update) = std::thread::scope(|scope| {
        let update = scope.spawn(|| {
            sender.request_with_timeout(session, runtime, command, Some(Duration::from_secs(5)))
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !adapter
            .dispatched()
            .iter()
            .any(|action| matches!(action, CanonicalAction::Reconfigure { .. }))
        {
            if Instant::now() >= deadline {
                confirmation.stop();
                panic!("configuration did not reach the host");
            }
            std::thread::yield_now();
        }
        let (finished, completion) = crossbeam_channel::bounded(1);
        let stop_client = stopper.clone();
        let stop = scope.spawn(move || {
            let began = Instant::now();
            let result = stop_client.request_with_timeout(
                session,
                runtime,
                Command::Cancel,
                Some(Duration::from_secs(5)),
            );
            let _ = finished.send((began.elapsed(), result));
        });
        let early = completion.recv_timeout(Duration::from_millis(250)).ok();
        let cancelled_before_activation = !adapter.cancels().is_empty();
        // Release before joining or asserting, including on the old blocked path.
        let (revision, target) = adapter
            .dispatched()
            .into_iter()
            .find_map(|action| match action {
                CanonicalAction::Reconfigure { revision, target } => Some((revision, target)),
                _ => None,
            })
            .unwrap();
        adapter.emit(ProviderEvent::ConfigActivated {
            revision,
            boundary: waku_runtime::contract::ActivationBoundary::Immediate,
            effective: target,
        });
        let was_early = early.is_some();
        let (elapsed, stopped) =
            early.unwrap_or_else(|| completion.recv_timeout(Duration::from_secs(5)).unwrap());
        let update = update.join().unwrap();
        stop.join().unwrap();
        assert!(matches!(stopped.unwrap(), ResponsePayload::Ack));
        (
            was_early,
            elapsed,
            cancelled_before_activation,
            update.unwrap(),
        )
    });
    let prompt_executed = adapter
        .dispatched()
        .iter()
        .any(|action| matches!(action, CanonicalAction::Submit { .. }));
    let update_applied = match update {
        ResponsePayload::OptionsApplied { applied } => Some(applied),
        ResponsePayload::Ack => None,
        other => panic!("unexpected update result: {other:?}"),
    };
    let interrupted = backend.task_state.lock().sessions[0]
        .turns
        .last()
        .is_some_and(|turn| turn.status == TurnStatus::Interrupted);
    drop(sender);
    drop(stopper);
    drop(fixture);
    Observation {
        early,
        stop_elapsed,
        cancelled_before_activation,
        prompt_executed,
        update_applied,
        interrupted,
    }
}

#[test]
fn stable_configuration_waits_do_not_hold_stop() {
    for configured in [false, true] {
        let probes: Vec<_> = (0..30)
            .map(|probe_id| probe(configured, probe_id % 2 == 0))
            .collect();
        let mut times: Vec<_> = probes.iter().map(|probe| probe.stop_elapsed).collect();
        times.sort();
        eprintln!(
            "stable configured_prompt={configured}: Stop p95={:?}, max={:?}",
            times[28], times[29]
        );
        assert!(
            probes.iter().all(|probe| probe.early),
            "Stop waited for provider activation"
        );
        assert!(probes.iter().all(|probe| probe.interrupted));
        if configured {
            assert!(
                probes.iter().all(|probe| !probe.prompt_executed),
                "a stopped configured prompt was submitted"
            );
        } else {
            assert!(
                probes.iter().all(|probe| probe.cancelled_before_activation),
                "running work was not interrupted during confirmation"
            );
            assert!(
                probes
                    .iter()
                    .all(|probe| probe.update_applied == Some(false))
            );
        }
    }
}
