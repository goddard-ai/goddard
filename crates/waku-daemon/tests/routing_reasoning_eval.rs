//! Live calibration, deliberately excluded from ordinary deterministic tests.
//! Set GODDARD_ROUTING_EVAL_ADDRESS and GODDARD_ROUTING_EVAL_TOKEN for a daemon
//! authorized to evaluate, then run:
//! mbx test -p waku-daemon --test routing_reasoning_eval -- --ignored --nocapture

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;
use waku_client::{Command, DaemonClient, ResponsePayload};
use waku_protocol::eval::EvalAnswer;
use waku_protocol::model::ProviderModelOption;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    kind: String,
    state: Value,
    expected: Vec<String>,
}

#[test]
#[ignore = "requires an authorized running daemon and live Jev backend; makes paid evaluations"]
fn reasoning_routing_calibration() {
    let address = std::env::var("GODDARD_ROUTING_EVAL_ADDRESS")
        .expect("set GODDARD_ROUTING_EVAL_ADDRESS to the evaluation daemon");
    let token = std::env::var("GODDARD_ROUTING_EVAL_TOKEN")
        .expect("set GODDARD_ROUTING_EVAL_TOKEN to an authorized daemon token");
    let client = DaemonClient::connect(&address, token).expect("connect to evaluation daemon");
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/routing_reasoning.json"))
        .expect("valid routing evaluation cases");
    let efforts = ["none", "minimal", "low", "medium", "high", "xhigh", "max"]
        .map(|id| ProviderModelOption::new(id, id));
    let mut failures = Vec::new();
    for case in &cases {
        let (key, questions) = match case.kind.as_str() {
            "start" => ("class", waku_daemon::routing::routing_questions()),
            "effort" => (
                "effort",
                BTreeMap::from([(
                    "effort".into(),
                    waku_client::routing::turn_effort_question(&efforts),
                )]),
            ),
            "handoff" => (
                "remaining_work",
                waku_client::routing::model_handoff_questions(),
            ),
            _ => panic!("unknown case kind"),
        };
        let payload = client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::Evaluate {
                    state: case.state.clone(),
                    questions,
                    feature: Some(format!("route-calibration-{}", case.kind)),
                    timeout_secs: Some(30),
                },
            )
            .expect("daemon evaluation request failed");
        let ResponsePayload::Evaluation { evaluation } = payload else {
            panic!("daemon did not return an evaluation");
        };
        let Some(EvalAnswer::Choice {
            choice,
            confidence,
            probabilities,
        }) = evaluation.answers.get(key)
        else {
            panic!("missing choice for {}", case.name);
        };
        let passed = case.expected.contains(choice);
        println!(
            "{}: {} -> {choice}; expected {:?}; confidence {confidence:?}; probabilities {probabilities:?}; model {}",
            if passed { "PASS" } else { "FAIL" },
            case.name,
            case.expected,
            evaluation.model,
        );
        if !passed {
            failures.push(case.name.as_str());
        }
    }
    assert!(failures.is_empty(), "routing mismatches: {failures:?}");
}
