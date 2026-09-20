use anyhow::{Context as _, anyhow, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::WireDriverEvent;
use crate::computer_use::{ComputerTarget, ComputerUsePhase, ComputerUseState};
use crate::model::{ActivityKind, DriverEvent, PermissionOption, UserInputQuestion};

pub fn decode_enum<T: DeserializeOwned>(value: &str) -> anyhow::Result<T> {
    serde_json::from_value(Value::String(value.to_owned()))
        .with_context(|| format!("invalid protocol enum value {value:?}"))
}

pub fn encode_enum<T: Serialize>(value: T) -> anyhow::Result<String> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("protocol enum did not serialize as a string"))
}

pub fn event_to_wire(event: DriverEvent) -> anyhow::Result<WireDriverEvent> {
    let (kind, payload) = match event {
        DriverEvent::RuntimeEventCursorAdvanced(_) => {
            bail!("client-only runtime cursors cannot be sent by the daemon")
        }
        DriverEvent::Connected { provider_cursor } => {
            ("connected", serde_json::to_value(provider_cursor)?)
        }
        DriverEvent::AgentPresetSelected(preset) => {
            ("agentPresetSelected", serde_json::to_value(preset)?)
        }
        DriverEvent::AutoTitleUpdated(title) => ("autoTitleUpdated", serde_json::to_value(title)?),
        DriverEvent::AvailableCommands(commands) => {
            ("availableCommands", serde_json::to_value(commands)?)
        }
        DriverEvent::TurnStarted => ("turnStarted", Value::Null),
        DriverEvent::TurnParked => ("turnParked", Value::Null),
        DriverEvent::TextDelta(text) => ("textDelta", Value::String(text)),
        DriverEvent::ReasoningDelta(text) => ("reasoningDelta", Value::String(text)),
        DriverEvent::Activity {
            id,
            kind,
            title,
            detail,
            complete,
        } => (
            "activity",
            json!({
                "id": id,
                "kind": kind,
                "title": title,
                "detail": detail,
                "complete": complete,
            }),
        ),
        DriverEvent::RichActivity(activity) => ("richActivity", serde_json::to_value(activity)?),
        DriverEvent::BackgroundWork(work) => ("backgroundWork", serde_json::to_value(work)?),
        DriverEvent::Permission {
            request_id,
            title,
            title_i18n,
            detail,
            detail_i18n,
            options,
        } => (
            "permission",
            json!({
                "requestId": request_id,
                "title": title,
                "titleI18n": title_i18n,
                "detail": detail,
                "detailI18n": detail_i18n,
                "options": options,
            }),
        ),
        DriverEvent::UserInputRequested {
            request_id,
            questions,
        } => (
            "userInputRequested",
            json!({
                "requestId": request_id,
                "questions": questions,
            }),
        ),
        DriverEvent::ComputerUseUpdated(state) => (
            "computerUseUpdated",
            serde_json::to_value(ComputerUseWire {
                target: state.target,
                phase: state.phase,
                visible: state.visible,
                image_url: state.image_url,
            })?,
        ),
        DriverEvent::PromptSubmitted {
            message,
            turn_id,
            message_id,
            sent_by_task,
            hidden,
        } => (
            "promptSubmitted",
            json!({
                "message": message,
                "turnId": turn_id,
                "messageId": message_id,
                "sentByTask": sent_by_task,
                "hidden": hidden,
            }),
        ),
        DriverEvent::SteerAccepted {
            message,
            sent_by_task,
        } => (
            "steerAccepted",
            json!({ "message": message, "sentByTask": sent_by_task }),
        ),
        DriverEvent::QueuedMessagesChanged { messages } => (
            "queuedMessagesChanged",
            json!({ "messages": messages }),
        ),
        DriverEvent::SteerRejected {
            message,
            reason,
            reason_i18n,
        } => (
            "steerRejected",
            json!({ "message": message, "reason": reason, "reasonI18n": reason_i18n }),
        ),
        DriverEvent::UsageUpdated {
            context_tokens,
            context_window,
        } => (
            "usageUpdated",
            json!({
                "contextTokens": context_tokens,
                "contextWindow": context_window,
            }),
        ),
        DriverEvent::PlanUsageUpdated(usage) => ("planUsageUpdated", serde_json::to_value(usage)?),
        DriverEvent::GoalUpdated(goal) => ("goalUpdated", serde_json::to_value(goal)?),
        DriverEvent::ProjectMap(status) => ("projectMap", serde_json::to_value(status)?),
        DriverEvent::SandboxSetup(status) => ("sandboxSetup", serde_json::to_value(status)?),
        DriverEvent::TurnFinished {
            success,
            summary,
            summary_i18n,
        } => (
            "turnFinished",
            json!({ "success": success, "summary": summary, "summaryI18n": summary_i18n }),
        ),
        DriverEvent::LocalizedError { message, i18n } => (
            "localizedError",
            json!({ "message": message, "i18n": i18n }),
        ),
        DriverEvent::Error(error) => ("error", Value::String(error)),
        DriverEvent::ProcessExited => ("processExited", Value::Null),
    };
    Ok(WireDriverEvent::new(kind, payload))
}

pub fn event_from_wire(event: WireDriverEvent) -> anyhow::Result<DriverEvent> {
    let payload = event.payload;
    Ok(match event.kind.as_str() {
        "connected" => DriverEvent::Connected {
            provider_cursor: serde_json::from_value(payload)?,
        },
        "agentPresetSelected" => DriverEvent::AgentPresetSelected(serde_json::from_value(payload)?),
        "autoTitleUpdated" => DriverEvent::AutoTitleUpdated(serde_json::from_value(payload)?),
        "availableCommands" => DriverEvent::AvailableCommands(serde_json::from_value(payload)?),
        "turnStarted" => DriverEvent::TurnStarted,
        "turnParked" => DriverEvent::TurnParked,
        "textDelta" => DriverEvent::TextDelta(serde_json::from_value(payload)?),
        "reasoningDelta" => DriverEvent::ReasoningDelta(serde_json::from_value(payload)?),
        "activity" => {
            let activity: ActivityWire = serde_json::from_value(payload)?;
            DriverEvent::Activity {
                id: activity.id,
                kind: activity.kind,
                title: activity.title,
                detail: activity.detail,
                complete: activity.complete,
            }
        }
        "richActivity" => DriverEvent::RichActivity(serde_json::from_value(payload)?),
        "backgroundWork" => DriverEvent::BackgroundWork(serde_json::from_value(payload)?),
        "permission" => {
            let permission: PermissionWire = serde_json::from_value(payload)?;
            DriverEvent::Permission {
                request_id: permission.request_id,
                title: permission.title,
                title_i18n: permission.title_i18n,
                detail: permission.detail,
                detail_i18n: permission.detail_i18n,
                options: permission.options,
            }
        }
        "userInputRequested" => {
            let request: UserInputWire = serde_json::from_value(payload)?;
            DriverEvent::UserInputRequested {
                request_id: request.request_id,
                questions: request.questions,
            }
        }
        "computerUseUpdated" => {
            let state: ComputerUseWire = serde_json::from_value(payload)?;
            DriverEvent::ComputerUseUpdated(ComputerUseState {
                target: state.target,
                phase: state.phase,
                visible: state.visible,
                image_url: state.image_url,
            })
        }
        "promptSubmitted" => {
            let submitted: SubmittedPromptWire = serde_json::from_value(payload)?;
            DriverEvent::PromptSubmitted {
                message: submitted.message,
                turn_id: submitted.turn_id,
                message_id: submitted.message_id,
                sent_by_task: submitted.sent_by_task,
                hidden: submitted.hidden,
            }
        }
        "steerAccepted" => {
            let steer: AcceptedSteerWire = serde_json::from_value(payload)?;
            DriverEvent::SteerAccepted {
                message: steer.message,
                sent_by_task: steer.sent_by_task,
            }
        }
        "queuedMessagesChanged" => {
            #[derive(Deserialize)]
            struct QueueWire {
                messages: Vec<crate::model::QueuedMessage>,
            }
            let queue: QueueWire = serde_json::from_value(payload)?;
            DriverEvent::QueuedMessagesChanged {
                messages: queue.messages,
            }
        }
        "steerRejected" => {
            let steer: RejectedSteerWire = serde_json::from_value(payload)?;
            DriverEvent::SteerRejected {
                message: steer.message,
                reason: steer.reason,
                reason_i18n: steer.reason_i18n,
            }
        }
        "usageUpdated" => {
            let usage: UsageWire = serde_json::from_value(payload)?;
            DriverEvent::UsageUpdated {
                context_tokens: usage.context_tokens,
                context_window: usage.context_window,
            }
        }
        "planUsageUpdated" => DriverEvent::PlanUsageUpdated(serde_json::from_value(payload)?),
        "goalUpdated" => DriverEvent::GoalUpdated(serde_json::from_value(payload)?),
        "projectMap" => DriverEvent::ProjectMap(serde_json::from_value(payload)?),
        "sandboxSetup" => DriverEvent::SandboxSetup(serde_json::from_value(payload)?),
        "turnFinished" => {
            let finished: TurnFinishedWire = serde_json::from_value(payload)?;
            DriverEvent::TurnFinished {
                success: finished.success,
                summary: finished.summary,
                summary_i18n: finished.summary_i18n,
            }
        }
        "localizedError" => {
            let error: LocalizedErrorWire = serde_json::from_value(payload)?;
            DriverEvent::LocalizedError {
                message: error.message,
                i18n: error.i18n,
            }
        }
        "error" => DriverEvent::Error(serde_json::from_value(payload)?),
        "processExited" => DriverEvent::ProcessExited,
        kind => bail!("daemon sent an unsupported driver event {kind:?}"),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmittedPromptWire {
    message: String,
    turn_id: Uuid,
    message_id: Uuid,
    #[serde(default)]
    sent_by_task: Option<Uuid>,
    #[serde(default)]
    hidden: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivityWire {
    id: Option<String>,
    kind: ActivityKind,
    title: String,
    detail: Option<String>,
    complete: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PermissionWire {
    request_id: String,
    title: String,
    #[serde(default)]
    title_i18n: Option<crate::protocol::WireTranslation>,
    detail: String,
    #[serde(default)]
    detail_i18n: Option<crate::protocol::WireTranslation>,
    options: Vec<PermissionOption>,
}

#[derive(Deserialize)]
struct LocalizedErrorWire {
    message: String,
    i18n: crate::protocol::WireTranslation,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserInputWire {
    request_id: String,
    questions: Vec<UserInputQuestion>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ComputerUseWire {
    target: Option<ComputerTarget>,
    phase: ComputerUsePhase,
    visible: bool,
    image_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcceptedSteerWire {
    message: String,
    #[serde(default)]
    sent_by_task: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RejectedSteerWire {
    message: String,
    reason: String,
    #[serde(default)]
    reason_i18n: Option<crate::protocol::WireTranslation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageWire {
    context_tokens: Option<u64>,
    context_window: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnFinishedWire {
    success: bool,
    summary: Option<String>,
    #[serde(default)]
    summary_i18n: Option<crate::protocol::WireTranslation>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        SandboxSetupStatus, ThreadGoal, ThreadGoalStatus, UserInputOption, UserInputQuestion,
    };

    #[test]
    fn goal_updates_round_trip_through_the_daemon_wire() {
        let wire = event_to_wire(DriverEvent::GoalUpdated(Some(ThreadGoal {
            objective: "Ship the feature".into(),
            status: ThreadGoalStatus::UsageLimited,
            token_budget: Some(50_000),
            tokens_used: 12_500,
            time_used_seconds: 90,
        })))
        .unwrap();
        assert_eq!(wire.kind, "goalUpdated");
        // The status spelling is Codex's own camelCase vocabulary.
        assert_eq!(wire.payload["status"], "usageLimited");

        let DriverEvent::GoalUpdated(Some(goal)) = event_from_wire(wire).unwrap() else {
            panic!("the event changed variants during its wire round trip");
        };
        assert_eq!(goal.objective, "Ship the feature");
        assert_eq!(goal.status, ThreadGoalStatus::UsageLimited);
        assert_eq!(goal.token_budget, Some(50_000));

        let cleared = event_to_wire(DriverEvent::GoalUpdated(None)).unwrap();
        assert!(matches!(
            event_from_wire(cleared).unwrap(),
            DriverEvent::GoalUpdated(None)
        ));
    }

    #[test]
    fn sandbox_setup_round_trips_through_the_daemon_wire() {
        let wire = event_to_wire(DriverEvent::SandboxSetup(
            SandboxSetupStatus::BuildingToolchain {
                toolchain: "node@lts".into(),
            },
        ))
        .unwrap();
        assert_eq!(wire.kind, "sandboxSetup");
        assert_eq!(wire.payload["state"], "buildingToolchain");

        let DriverEvent::SandboxSetup(SandboxSetupStatus::BuildingToolchain { toolchain }) =
            event_from_wire(wire).unwrap()
        else {
            panic!("the event changed variants during its wire round trip");
        };
        assert_eq!(toolchain, "node@lts");

        let ready = event_to_wire(DriverEvent::SandboxSetup(SandboxSetupStatus::Ready)).unwrap();
        assert!(matches!(
            event_from_wire(ready).unwrap(),
            DriverEvent::SandboxSetup(SandboxSetupStatus::Ready)
        ));
    }

    #[test]
    fn queued_messages_changed_round_trips_through_the_daemon_wire() {
        use crate::model::QueuedMessage;

        let sender = uuid::Uuid::new_v4();
        let agent = QueuedMessage::agent("parked agent prompt", Some(sender));
        let wire = event_to_wire(DriverEvent::QueuedMessagesChanged {
            messages: vec![agent.clone()],
        })
        .unwrap();
        assert_eq!(wire.kind, "queuedMessagesChanged");

        let DriverEvent::QueuedMessagesChanged { messages } = event_from_wire(wire).unwrap()
        else {
            panic!("the event changed variants during its wire round trip");
        };
        assert_eq!(messages, vec![agent]);

        let empty = event_to_wire(DriverEvent::QueuedMessagesChanged { messages: vec![] }).unwrap();
        let DriverEvent::QueuedMessagesChanged { messages } = event_from_wire(empty).unwrap()
        else {
            panic!("an empty snapshot failed its wire round trip");
        };
        assert!(messages.is_empty());
    }

    #[test]
    fn structured_user_input_round_trips_through_the_daemon_wire() {
        let wire = event_to_wire(DriverEvent::UserInputRequested {
            request_id: "request-1".into(),
            questions: vec![UserInputQuestion {
                id: "deployment".into(),
                header: "Environment".into(),
                question: "Where should this deploy?".into(),
                options: vec![UserInputOption {
                    label: "Preview".into(),
                    description: Some("Create a preview deployment".into()),
                }],
                multi_select: false,
            }],
        })
        .unwrap();
        assert_eq!(wire.kind, "userInputRequested");

        let DriverEvent::UserInputRequested {
            request_id,
            questions,
        } = event_from_wire(wire).unwrap()
        else {
            panic!("the event changed variants during its wire round trip");
        };
        assert_eq!(request_id, "request-1");
        assert_eq!(questions[0].id, "deployment");
        assert_eq!(questions[0].options[0].label, "Preview");
    }

    #[test]
    fn localized_events_round_trip_through_the_daemon_wire() {
        let i18n = crate::protocol::WireTranslation {
            key: "errors.provider_receive_prompt".into(),
            args: [("provider".to_owned(), "Amp".to_owned())]
                .into_iter()
                .collect(),
        };

        let wire = event_to_wire(DriverEvent::LocalizedError {
            message: "Amp stopped receiving the prompt".into(),
            i18n: i18n.clone(),
        })
        .unwrap();
        assert_eq!(wire.kind, "localizedError");
        assert_eq!(
            wire.payload["i18n"]["key"],
            "errors.provider_receive_prompt"
        );

        let DriverEvent::LocalizedError { message, i18n } = event_from_wire(wire).unwrap() else {
            panic!("the event changed variants during its wire round trip");
        };
        assert_eq!(message, "Amp stopped receiving the prompt");
        assert_eq!(i18n.key, "errors.provider_receive_prompt");
        assert_eq!(i18n.args["provider"], "Amp");
    }

    #[test]
    fn keyed_fields_round_trip_and_stay_optional() {
        let i18n = crate::protocol::WireTranslation {
            key: "permission.agent_asks_for_permission".into(),
            args: Default::default(),
        };
        let wire = event_to_wire(DriverEvent::Permission {
            request_id: "per_1".into(),
            title: "npm test".into(),
            title_i18n: None,
            detail: "The agent asks for permission".into(),
            detail_i18n: Some(i18n.clone()),
            options: vec![],
        })
        .unwrap();
        assert_eq!(
            wire.payload["detailI18n"]["key"],
            "permission.agent_asks_for_permission"
        );

        let DriverEvent::Permission {
            detail_i18n,
            title_i18n,
            ..
        } = event_from_wire(wire).unwrap()
        else {
            panic!("the event changed variants during its wire round trip");
        };
        assert!(title_i18n.is_none());
        assert_eq!(
            detail_i18n.unwrap().key,
            "permission.agent_asks_for_permission"
        );

        // A payload written by an older daemon carries no i18n fields at all
        // and must still decode.
        let legacy = crate::protocol::WireDriverEvent {
            kind: "permission".into(),
            payload: serde_json::json!({
                "requestId": "per_2",
                "title": "rm -rf *",
                "detail": "The agent asks for permission",
                "options": []
            }),
        };
        let DriverEvent::Permission {
            title_i18n,
            detail_i18n,
            ..
        } = event_from_wire(legacy).unwrap()
        else {
            panic!("the legacy permission payload failed to decode");
        };
        assert!(title_i18n.is_none() && detail_i18n.is_none());
    }

    #[test]
    fn keyed_turn_and_steer_fields_round_trip_and_stay_optional() {
        let i18n = crate::protocol::WireTranslation {
            key: "session.agent_ran_out_of_context".into(),
            args: Default::default(),
        };
        let wire = event_to_wire(DriverEvent::TurnFinished {
            success: false,
            summary: Some("The agent ran out of context".into()),
            summary_i18n: Some(i18n.clone()),
        })
        .unwrap();
        assert_eq!(
            wire.payload["summaryI18n"]["key"],
            "session.agent_ran_out_of_context"
        );
        let DriverEvent::TurnFinished { summary_i18n, .. } = event_from_wire(wire).unwrap() else {
            panic!("the event changed variants during its wire round trip");
        };
        assert!(summary_i18n.is_some());

        let wire = event_to_wire(DriverEvent::steer_rejected_keyed(
            "keep going".into(),
            (
                "Claude has no active turn".into(),
                crate::protocol::WireTranslation {
                    key: "errors.provider_no_active_turn".into(),
                    args: [("provider".to_owned(), "Claude".to_owned())]
                        .into_iter()
                        .collect(),
                },
            ),
        ))
        .unwrap();
        assert_eq!(
            wire.payload["reasonI18n"]["key"],
            "errors.provider_no_active_turn"
        );
        let DriverEvent::SteerRejected { reason_i18n, .. } = event_from_wire(wire).unwrap() else {
            panic!("the event changed variants during its wire round trip");
        };
        assert_eq!(reason_i18n.unwrap().args["provider"], "Claude");

        // Legacy payloads without the i18n fields decode with `None`.
        let legacy = crate::protocol::WireDriverEvent {
            kind: "steerRejected".into(),
            payload: serde_json::json!({"message": "go", "reason": "nope"}),
        };
        let DriverEvent::SteerRejected { reason_i18n, .. } = event_from_wire(legacy).unwrap()
        else {
            panic!("the legacy steer rejection failed to decode");
        };
        assert!(reason_i18n.is_none());
    }
}
