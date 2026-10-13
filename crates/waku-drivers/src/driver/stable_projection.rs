//! Product projection of adapter-owned native notifications. Provider I/O,
//! native replies and lifecycle authority remain in the stable host.

use super::{DriverEventSink, acp, codex};
use crate::model::{DriverEvent, PermissionOption, ProviderKind};
use crossbeam_channel::SendError;
use serde_json::Value;
use std::sync::Mutex;

#[derive(Default)]
pub(super) struct ProjectionSink(Mutex<Vec<DriverEvent>>);
impl DriverEventSink for ProjectionSink {
    fn send(&self, event: DriverEvent) -> Result<(), SendError<DriverEvent>> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

/// Reuses the legacy drivers' transcript parsers without owning a provider
/// connection or dispatching commands. One instance belongs to one event pump.
#[derive(Default)]
pub struct NativeEventProjection {
    codex: codex::StableProjection,
    acp: acp::StableProjection,
}
impl NativeEventProjection {
    pub fn set_codex_thread(&mut self, thread: Option<String>) {
        self.codex.set_thread(thread);
    }
    /// Native request copy and option identities; resolution stays host-owned.
    pub fn request(
        &self,
        provider: ProviderKind,
        id: &str,
        subject: &Value,
    ) -> Option<DriverEvent> {
        match provider {
            ProviderKind::Codex => self.codex.request(id, subject),
            ProviderKind::Devin if subject.get("toolCall").is_some() => {
                let options = subject
                    .get("options")?
                    .as_array()?
                    .iter()
                    .filter_map(|option| {
                        Some(PermissionOption {
                            id: option.get("optionId")?.as_str()?.into(),
                            label: option.get("name")?.as_str()?.into(),
                            label_i18n: None,
                            allow: matches!(
                                option.get("kind").and_then(Value::as_str),
                                Some("allow_once" | "allow_always")
                            ),
                        })
                    })
                    .collect();
                Some(DriverEvent::Permission {
                    request_id: id.into(),
                    title: subject
                        .pointer("/toolCall/title")
                        .and_then(Value::as_str)
                        .unwrap_or("Provider approval")
                        .into(),
                    detail: String::new(),
                    title_i18n: None,
                    detail_i18n: None,
                    options,
                })
            }
            _ => None,
        }
    }
    pub fn project(&mut self, provider: ProviderKind, delta: &Value) -> Vec<DriverEvent> {
        let sink = ProjectionSink::default();
        match provider {
            ProviderKind::Codex => self.codex.project(delta, &sink),
            ProviderKind::Devin => self.acp.project(delta, &sink),
            _ => {}
        }
        sink.0
            .into_inner()
            .unwrap()
            .into_iter()
            .filter(|event| {
                !matches!(
                    event,
                    DriverEvent::TurnStarted
                        | DriverEvent::TurnFinished { .. }
                        | DriverEvent::ProcessExited
                        | DriverEvent::Connected { .. }
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_streams_preserve_text_reasoning_and_tool_activity() {
        let mut projection = NativeEventProjection::default();
        projection.set_codex_thread(Some("thread-1".into()));
        projection.project(ProviderKind::Codex, &json!({"method":"turn/started", "params":{"threadId":"thread-1","turn":{"id":"turn-1"}}}));
        let text = projection.project(ProviderKind::Codex, &json!({"method":"item/agentMessage/delta", "params":{"threadId":"thread-1","turnId":"turn-1","itemId":"a","delta":"hello"}}));
        assert!(matches!(&text[..], [DriverEvent::TextDelta(text)] if text == "hello"));
        let reasoning = projection.project(ProviderKind::Codex, &json!({"method":"item/reasoning/textDelta", "params":{"threadId":"thread-1","turnId":"turn-1","itemId":"r","delta":"thinking"}}));
        assert!(
            matches!(&reasoning[..], [DriverEvent::ReasoningDelta(text)] if text == "thinking")
        );
        assert!(projection.project(ProviderKind::Codex, &json!({"method":"item/agentMessage/delta", "params":{"threadId":"foreign","turnId":"turn-2","delta":"must not appear"}})).is_empty());
        let text = projection.project(ProviderKind::Devin, &json!({"sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":"ACP text"}}));
        assert!(matches!(&text[..], [DriverEvent::TextDelta(text)] if text == "ACP text"));
        let thought = projection.project(ProviderKind::Devin, &json!({"sessionUpdate":"agent_thought_chunk", "content":{"type":"text","text":"ACP thought"}}));
        assert!(
            matches!(&thought[..], [DriverEvent::ReasoningDelta(text)] if text == "ACP thought")
        );
        let tool = projection.project(ProviderKind::Devin, &json!({"sessionUpdate":"tool_call", "toolCallId":"tool-1", "title":"Read file", "kind":"read", "status":"in_progress"}));
        assert!(tool.iter().any(|event| matches!(
            event,
            DriverEvent::Activity { .. } | DriverEvent::RichActivity(_)
        )));
        let finish = projection.project(ProviderKind::Devin, &json!({"sessionUpdate":"tool_call_update", "toolCallId":"tool-1", "status":"completed"}));
        assert!(finish.iter().any(|event| matches!(
            event,
            DriverEvent::Activity { complete: true, .. }
                | DriverEvent::RichActivity(crate::model::ActivityItem { complete: true, .. })
        )));
    }
    #[test]
    fn native_permission_cards_keep_stable_ids_and_deny_options() {
        let projection = NativeEventProjection::default();
        let id = "\"native-id\"";
        let codex = projection.request(ProviderKind::Codex, id, &json!({
            "id": "native-id", "method": "item/commandExecution/requestApproval", "params": {"command": "read file"},
        })).unwrap();
        assert!(
            matches!(codex, DriverEvent::Permission { request_id, options, .. }
            if request_id == id && options.iter().any(|option| option.id == "decline" && !option.allow))
        );
        let devin = projection
            .request(
                ProviderKind::Devin,
                "acp-original",
                &json!({
                    "toolCall": {"toolCallId": "tool"}, "options": [
                        {"optionId": "yes", "name": "Allow once", "kind": "allow_once"},
                        {"optionId": "no", "name": "Deny", "kind": "reject_once"},
                    ],
                }),
            )
            .unwrap();
        assert!(
            matches!(devin, DriverEvent::Permission { request_id, options, .. }
            if request_id == "acp-original" && options.len() == 2 && options[0].allow && !options[1].allow)
        );
    }
}
