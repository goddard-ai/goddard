use super::*;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

#[test]
fn copilot_settings_require_an_applied_model_not_a_deferred_acknowledgement() {
    for outcome in [
        "applied",
        "refused",
        "deferred",
        "confirmation_required",
        "disconnected",
    ] {
        let root =
            std::env::temp_dir().join(format!("configured-copilot-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let binary = root.join("copilot-fixture");
        let trace = root.join("trace.jsonl");
        std::fs::write(
            &binary,
            r##"#!/usr/bin/env python3
import json, pathlib, sys
trace = pathlib.Path(__file__).with_name('trace.jsonl')
model = 'initial-model'
def send(value):
    body = json.dumps(value).encode()
    sys.stdout.buffer.write(('Content-Length: %d\r\n\r\n' % len(body)).encode() + body)
    sys.stdout.buffer.flush()
while True:
    line = sys.stdin.buffer.readline()
    if not line:
        break
    size = int(line.decode().split(':', 1)[1])
    while sys.stdin.buffer.readline().strip():
        pass
    request = json.loads(sys.stdin.buffer.read(size))
    method, params = request.get('method'), request.get('params', {})
    if 'id' not in request:
        continue
    with trace.open('a') as out:
        out.write(json.dumps({'method': method}) + '\n')
    result, error = {}, None
    if method == 'connect':
        result = {'ok': True, 'protocolVersion': 3, 'version': 'fixture'}
    elif method == 'session.create':
        result = {'sessionId': params['sessionId']}
    elif method == 'session.model.switchTo':
        outcome = params['modelId']
        with trace.open('a') as out:
            out.write(json.dumps({'change': outcome}) + '\n')
        if outcome == 'disconnected':
            break
        if outcome == 'refused':
            error = {'code': -32602, 'message': 'fixture refused the selected model'}
        elif outcome == 'applied':
            model = outcome
            result = {'modelId': model, 'status': 'completed', 'deferred': False}
        else:
            result = {'modelId': model, 'status': outcome, 'deferred': outcome == 'deferred'}
    elif method == 'session.send':
        with trace.open('a') as out:
            out.write(json.dumps({'executed_model': model}) + '\n')
        result = {'messageId': 'fixture-prompt'}
    response = {'jsonrpc': '2.0', 'id': request['id']}
    response['error' if error else 'result'] = error if error else result
    send(response)
    if method == 'runtime.shutdown':
        break
"##,
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let selected = |model: &str| SessionOptions {
            mode: crate::model::RuntimeMode::FullAccess,
            model: Some(model.into()),
            reasoning_effort: None,
            service_tier: None,
            context_window: None,
        };
        let (events, received) = test_event_channel();
        let driver = DriverHandle::from_control(Arc::new(
            copilot::CopilotDriver::start(
                launch_options(binary, root.clone(), "initial-model"),
                events,
            )
            .unwrap(),
        ))
        .with_initial_options(selected("initial-model"));
        let prepared = driver.prepare_prompt_options(selected(outcome));
        let applied = prepared.is_ok();
        if applied {
            driver.prompt("use only my confirmed selection".into());
        }
        drop(prepared);
        let deadline = Instant::now() + Duration::from_secs(5);
        let observed = loop {
            let text = std::fs::read_to_string(&trace).unwrap_or_default();
            if (!applied && text.contains("change")) || text.contains("executed_model") {
                break text;
            }
            if Instant::now() >= deadline {
                let events: Vec<_> = received.try_iter().collect();
                panic!("Copilot fixture did not reach its boundary: {text}; events={events:?}");
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        drop(driver);
        let deadline = Instant::now() + Duration::from_secs(10);
        while let Ok(event) =
            received.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            if matches!(event, crate::model::DriverEvent::ProcessExited) {
                break;
            }
        }
        assert_eq!(
            applied,
            outcome == "applied",
            "Copilot outcome {outcome}: {observed}"
        );
        assert_eq!(
            observed.contains("executed_model"),
            outcome == "applied",
            "an unconfirmed setting fell through to a prompt: {observed}"
        );
        if applied {
            assert!(observed.contains("\"executed_model\": \"applied\""));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn launch_options(
    binary: std::path::PathBuf,
    cwd: std::path::PathBuf,
    model: &str,
) -> DriverStartOptions {
    DriverStartOptions {
        binary,
        cwd,
        mode: crate::model::RuntimeMode::FullAccess,
        model: Some(model.into()),
        reasoning_effort: None,
        service_tier: None,
        context_window: None,
        agent_preset: None,
        computer_use_enabled: false,
        agent: None,
        read_own_transcript: false,
        subagents: None,
        computer_use_runtime: None,
        mcp_servers: Vec::new(),
        http_mcp_capability_recorder: None,
        provider_cursor: None,
        eval: None,
        sandbox: None,
        allow_model_fallback: false,
        ephemeral: false,
    }
}

#[test]
fn claude_rejects_unsupported_service_tier_before_starting_a_provider() {
    let root = std::env::temp_dir().join(format!("configured-tier-{}", uuid::Uuid::new_v4()));
    let mut launch = launch_options(root.join("must-not-start"), root, "initial-model");
    launch.service_tier = Some("fast".into());
    let (events, _received) = test_event_channel();
    let error = claude::ClaudeDriver::start(launch, events)
        .err()
        .expect("an unsupported service tier must fail before launch");
    assert!(error.to_string().contains("service tier selection"));
}

#[test]
fn acp_startup_refusal_cannot_fall_through_to_a_prompt() {
    for seed_fresh_options in [false, true] {
        let root =
            std::env::temp_dir().join(format!("configured-acp-startup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let binary = root.join("acp-fixture");
        std::fs::write(&binary, include_str!("../fixtures/configured_acp.py")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut launch = launch_options(binary, root.clone(), "missing-model");
        launch.mode = crate::model::RuntimeMode::Ask;
        // Every production daemon launch uses strict model selection.
        launch.allow_model_fallback = false;
        let selected = SessionOptions {
            mode: crate::model::RuntimeMode::Ask,
            model: Some("missing-model".into()),
            reasoning_effort: None,
            service_tier: None,
            context_window: None,
        };
        let (events, received) = test_event_channel();
        let mut driver = DriverHandle::from_control(Arc::new(
            acp::AcpDriver::start(crate::model::ProviderKind::Kimi, launch, events).unwrap(),
        ));
        if seed_fresh_options {
            driver = driver.with_initial_options(selected.clone());
        }
        loop {
            match received
                .recv_timeout(Duration::from_secs(10))
                .expect("ACP fixture connects")
            {
                crate::model::DriverEvent::Connected { .. } => break,
                crate::model::DriverEvent::Error(error) => {
                    panic!("ACP fixture failed before selection: {error}")
                }
                _ => {}
            }
        }
        let prepared = driver.prepare_prompt_options(selected);
        if prepared.is_ok() {
            driver.prompt("use only the selected model".into());
        }
        drop(prepared);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut refusal = false;
        let mut exited = false;
        while let Ok(event) =
            received.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            match event {
                crate::model::DriverEvent::Error(_)
                | crate::model::DriverEvent::LocalizedError { .. } => refusal = true,
                crate::model::DriverEvent::ProcessExited => {
                    exited = true;
                    break;
                }
                _ => {}
            }
        }
        let observed = std::fs::read_to_string(root.join("trace.jsonl")).unwrap_or_default();
        driver.begin_shutdown();
        drop(driver);
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            refusal && exited,
            "startup must fail explicitly and close the command owner"
        );
        assert!(
            observed.is_empty(),
            "a refused model used the provider's previous model: {observed}"
        );
    }
}

#[test]
fn deepseek_http_refusal_and_partial_update_prevent_stale_prompt_execution() {
    for target in [
        "refused-model",
        "partial-model",
        "confirmed-model",
        "unsupported-tier",
        "unsupported-window",
    ] {
        let root =
            std::env::temp_dir().join(format!("configured-deepseek-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let binary = root.join("deepseek-fixture");
        std::fs::write(&binary, include_str!("../fixtures/configured_deepseek.py")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let selected = |model: &str| SessionOptions {
            mode: if model == "partial-model" {
                crate::model::RuntimeMode::Ask
            } else {
                crate::model::RuntimeMode::FullAccess
            },
            model: Some(format!("local/{model}")),
            reasoning_effort: None,
            service_tier: (model == "unsupported-tier").then(|| "fast".to_owned()),
            context_window: (model == "unsupported-window").then(|| "1m".to_owned()),
        };
        let (events, _received) = test_event_channel();
        let driver = DriverHandle::from_control(Arc::new(
            deepseek::DeepSeekDriver::start(
                launch_options(binary, root.clone(), "local/initial-model"),
                events,
            )
            .unwrap(),
        ))
        .with_initial_options(selected("initial-model"));
        let prepared = driver.prepare_prompt_options(selected(target));
        let applied = prepared.is_ok();
        if applied {
            driver.prompt("use the confirmed model".into());
        }
        drop(prepared);
        let deadline = Instant::now() + Duration::from_secs(5);
        let observed = loop {
            let text = std::fs::read_to_string(root.join("trace.jsonl")).unwrap_or_default();
            if (!applied && (target.starts_with("unsupported-") || text.contains(target)))
                || text.contains("executed_model")
            {
                break text;
            }
            assert!(
                Instant::now() < deadline,
                "Harness fixture never reached its boundary: {text}"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        let pid: i32 = std::fs::read_to_string(root.join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        drop(driver);
        let deadline = Instant::now() + Duration::from_secs(7);
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(
                Instant::now() < deadline,
                "local Harness process was not cleaned up"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            applied,
            target == "confirmed-model",
            "Harness {target}: {observed}"
        );
        assert_eq!(
            observed.contains("executed_model"),
            target == "confirmed-model",
            "unconfirmed settings fell through to a prompt: {observed}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn a_provider_refusal_must_prevent_a_prompt_using_stale_settings() {
    let root = std::env::temp_dir().join(format!("configured-options-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("claude-fixture");
    let trace = root.join("trace.jsonl");
    std::fs::write(
        &binary,
        r##"#!/usr/bin/env python3
import json, pathlib, sys
trace = pathlib.Path(__file__).with_name('trace.jsonl')
model = sys.argv[sys.argv.index('--model') + 1]
for line in sys.stdin:
    value = json.loads(line)
    if value.get('type') == 'control_request':
        request = value['request']
        if request.get('subtype') == 'set_model':
            refused = request['model'] == 'refused-model'
            if not refused:
                model = request['model']
            with trace.open('a') as out:
                out.write(json.dumps({'change': request['model'], 'refused': refused}) + '\n')
            response = {'subtype': 'error' if refused else 'success',
                        'request_id': value['request_id']}
            if refused:
                response['error'] = 'fixture refused the selected model'
            print(json.dumps({'type': 'control_response', 'response': response}), flush=True)
    elif value.get('type') == 'user':
        with trace.open('a') as out:
            out.write(json.dumps({'executed_model': model}) + '\n')
        print(json.dumps({'type': 'result', 'subtype': 'success', 'result': 'done'}), flush=True)
"##,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let selected = |model: &str| SessionOptions {
        mode: crate::model::RuntimeMode::FullAccess,
        model: Some(model.into()),
        reasoning_effort: None,
        service_tier: None,
        context_window: None,
    };
    let (events, _received) = test_event_channel();
    let driver = DriverHandle::from_control(Arc::new(
        claude::ClaudeDriver::start(
            launch_options(binary, root.clone(), "initial-model"),
            events,
        )
        .unwrap(),
    ))
    .with_initial_options(selected("initial-model"));
    let mut unsupported = selected("initial-model");
    unsupported.service_tier = Some("fast".into());
    let unsupported = driver.prepare_prompt_options(unsupported);
    let unsupported_rejected = unsupported.is_err();
    if !unsupported_rejected {
        driver.prompt("do not run without the selected service tier".into());
    }
    drop(unsupported);
    let prepared = driver.prepare_prompt_options(selected("refused-model"));
    let refused = prepared.is_err();
    if !refused {
        driver.prompt("run only under my selected model".into());
    }
    drop(prepared);
    let deadline = Instant::now() + Duration::from_secs(5);
    let observed = loop {
        let text = std::fs::read_to_string(&trace).unwrap_or_default();
        if (refused && text.contains("refused-model")) || text.contains("executed_model") {
            break text;
        }
        if Instant::now() >= deadline {
            driver.begin_shutdown();
            panic!("the local provider fixture did not reach its settings boundary: {text}");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let confirmed = driver.prepare_prompt_options(selected("confirmed-model"));
    let accepted = confirmed.is_ok();
    if accepted {
        driver.prompt("run under the confirmed model".into());
    }
    drop(confirmed);
    let success_deadline = Instant::now() + Duration::from_secs(5);
    let successful = loop {
        let text = std::fs::read_to_string(&trace).unwrap_or_default();
        if !accepted || text.contains("\"executed_model\": \"confirmed-model\"") {
            break text;
        }
        if Instant::now() >= success_deadline {
            driver.begin_shutdown();
            panic!("the confirmed model never executed: {text}");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    driver.begin_shutdown();
    drop(driver);
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        unsupported_rejected,
        "an ignored service tier admitted a prompt"
    );
    assert!(
        refused,
        "provider refused the selected settings, but prompt admission succeeded: {observed}"
    );
    assert!(
        !observed.contains("executed_model"),
        "a refused selection executed a stale prompt: {observed}"
    );
    assert!(
        accepted,
        "a confirmed selection must still work: {successful}"
    );
    let executions: Vec<_> = successful
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter_map(|value| {
            value
                .get("executed_model")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(executions, ["confirmed-model"]);
}

#[test]
fn pi_settings_refusal_and_partial_update_never_execute_a_stale_prompt() {
    for flavor in [pi::PiFlavor::Pi, pi::PiFlavor::OhMyPi] {
        for (model, effort, expected) in [
            ("fixture/refused-model", None, false),
            ("fixture/confirmed-model", Some("refused-effort"), false),
            ("fixture/confirmed-model", Some("high"), true),
            ("fixture/unsupported-tier", None, false),
            ("fixture/unsupported-window", None, false),
        ] {
            let root = std::env::temp_dir().join(format!("pi-options-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let binary = root.join("pi-fixture");
            let trace = root.join("trace.jsonl");
            std::fs::write(&binary, r##"#!/usr/bin/env python3
import json, pathlib, sys
trace = pathlib.Path(__file__).with_name('trace.jsonl')
model, effort = 'initial-model', None
for line in sys.stdin:
    value = json.loads(line)
    kind = value.get('type')
    refused = (kind == 'set_model' and value['modelId'] == 'refused-model') or (kind == 'set_thinking_level' and value['level'] == 'refused-effort')
    if kind == 'set_model' and not refused:
        model = value['modelId']
    if kind == 'set_thinking_level' and not refused:
        effort = value['level']
    if kind in ('set_model', 'set_thinking_level', 'prompt'):
        with trace.open('a') as out:
            record = {'command': kind, 'refused': refused}
            if kind == 'prompt':
                record.update(executed_model=model, executed_effort=effort)
            out.write(json.dumps(record) + '\n')
    data = {'sessionId': 'local-fixture'} if kind == 'get_state' else {}
    print(json.dumps({'type': 'response', 'id': value.get('id'), 'command': kind,
                      'success': not refused, 'data': data, 'error': 'fixture refused settings' if refused else None}), flush=True)
"##).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
            let (events, received) = test_event_channel();
            let driver = DriverHandle::from_control(Arc::new(
                pi::PiDriver::start(
                    flavor,
                    launch_options(binary, root.clone(), "fixture/initial-model"),
                    events,
                )
                .unwrap(),
            ))
            .with_initial_options(SessionOptions {
                mode: crate::model::RuntimeMode::FullAccess,
                model: Some("fixture/initial-model".into()),
                reasoning_effort: None,
                service_tier: None,
                context_window: None,
            });
            loop {
                match received.recv_timeout(Duration::from_secs(5)).unwrap() {
                    crate::model::DriverEvent::Connected { .. } => break,
                    crate::model::DriverEvent::Error(error) => {
                        panic!("fixture startup failed: {error}")
                    }
                    _ => {}
                }
            }
            let prepared = driver.prepare_prompt_options(SessionOptions {
                mode: crate::model::RuntimeMode::FullAccess,
                model: Some(model.into()),
                reasoning_effort: effort.map(str::to_owned),
                service_tier: model
                    .ends_with("unsupported-tier")
                    .then(|| "fast".to_owned()),
                context_window: model
                    .ends_with("unsupported-window")
                    .then(|| "1m".to_owned()),
            });
            let accepted = prepared.is_ok();
            if accepted {
                driver.prompt("use only the selected configuration".into());
            }
            drop(prepared);
            let deadline = Instant::now() + Duration::from_secs(5);
            let observed = loop {
                let text = std::fs::read_to_string(&trace).unwrap_or_default();
                if (accepted && text.contains("executed_model"))
                    || (!accepted
                        && (model.contains("unsupported-") || text.contains("\"refused\": true")))
                {
                    break text;
                }
                assert!(Instant::now() < deadline, "fixture did not settle: {text}");
                std::thread::sleep(Duration::from_millis(5));
            };
            driver.begin_shutdown();
            drop(driver);
            std::fs::remove_dir_all(root).unwrap();
            assert_eq!(
                accepted, expected,
                "{flavor:?}: model={model} effort={effort:?}: {observed}"
            );
            let executions: Vec<_> = observed
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .filter(|value| value.get("executed_model").is_some())
                .collect();
            if expected {
                assert_eq!(executions.len(), 1);
                assert_eq!(executions[0]["executed_model"], "confirmed-model");
                assert_eq!(executions[0]["executed_effort"], "high");
            } else {
                assert!(
                    executions.is_empty(),
                    "a refused or partial update executed: {observed}"
                );
            }
        }
    }
}

#[test]
fn configured_options_reject_unsupported_pi_and_deepseek_settings_before_launch() {
    for adapter in ["pi", "omp", "deepseek"] {
        for context_window in [false, true] {
            let mut options = launch_options(
                PathBuf::from("/missing/never-start-this-provider"),
                std::env::temp_dir(),
                "fixture/initial-model",
            );
            if context_window {
                options.context_window = Some("1m".into());
            } else {
                options.service_tier = Some("fast".into());
            }
            let (events, _received) = test_event_channel();
            let result = match adapter {
                "pi" => pi::PiDriver::start(pi::PiFlavor::Pi, options, events).map(|_| ()),
                "omp" => pi::PiDriver::start(pi::PiFlavor::OhMyPi, options, events).map(|_| ()),
                "deepseek" => deepseek::DeepSeekDriver::start(options, events).map(|_| ()),
                _ => unreachable!(),
            };
            let error = result
                .expect_err("unsupported settings must fail before launch")
                .to_string();
            let expected = if context_window {
                "context window selection is not supported"
            } else {
                "service tier selection is not supported"
            };
            assert!(error.contains(expected), "{adapter}: {error}");
        }
    }
}
