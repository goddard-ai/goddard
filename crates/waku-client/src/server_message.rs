//! Catalog responses bypass Serde's nested internally-tagged enum buffers.
//! Borrow only JSON boundaries, then stream rows directly into their DTOs.
use std::borrow::Cow;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::value::RawValue;
use uuid::Uuid;
use waku_protocol::model::{AgentSession, Project};
use waku_protocol::{ResponseOutcome, ResponsePayload, ServerMessage};

pub(super) fn decode(text: &str) -> serde_json::Result<ServerMessage> {
    // Keep the protocol decoder authoritative for every other message and for
    // malformed catalog responses. No wire format or DTO defaults change.
    if let Some(message) = catalog_response(text) {
        return Ok(message);
    }
    serde_json::from_str(text)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope<'a> {
    #[serde(rename = "type", borrow)]
    kind: Cow<'a, str>,
    request_id: Uuid,
    #[serde(borrow)]
    outcome: Outcome<'a>,
}

#[derive(Deserialize)]
struct Outcome<'a> {
    #[serde(borrow)]
    status: Cow<'a, str>,
    #[serde(borrow)]
    payload: &'a RawValue,
}

// Match ResponsePayload::TaskState's fields and defaults. The parity tests
// below compare against the protocol decoder, including malformed inputs.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog<'a> {
    #[serde(rename = "type", borrow)]
    kind: Cow<'a, str>,
    projects: Vec<Project>,
    sessions: Vec<AgentSession>,
    default_cwd: PathBuf,
    projectless_root: Option<PathBuf>,
}

fn catalog_response(text: &str) -> Option<ServerMessage> {
    let envelope: Envelope<'_> = serde_json::from_str(text).ok()?;
    if envelope.kind != "response" {
        return None;
    }
    if envelope.outcome.status != "ok" {
        return None;
    }
    let catalog: Catalog<'_> = serde_json::from_str(envelope.outcome.payload.get()).ok()?;
    if catalog.kind != "taskState" {
        return None;
    }
    Some(ServerMessage::Response {
        request_id: envelope.request_id,
        outcome: ResponseOutcome::Ok {
            payload: ResponsePayload::TaskState {
                projects: catalog.projects,
                sessions: catalog.sessions,
                default_cwd: catalog.default_cwd,
                projectless_root: catalog.projectless_root,
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::model::ProviderKind;

    fn check_parity(text: &str) {
        let actual = decode(text);
        let expected = serde_json::from_str::<ServerMessage>(text);
        assert_eq!(actual.is_ok(), expected.is_ok(), "{text}");
        if let (Ok(actual), Ok(expected)) = (actual, expected) {
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
    }

    fn response(payload: &str) -> String {
        format!(
            r#"{{"outcome":{{"payload":{payload},"status":"ok"}},"requestId":"00000000-0000-0000-0000-000000000001","type":"response"}}"#
        )
    }

    #[test]
    fn catalog_decoder_matches_protocol_for_large_rows_and_legacy_defaults() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.title = "quotes \" and unicode λ".into();
        let message = ServerMessage::Response {
            request_id: Uuid::new_v4(),
            outcome: ResponseOutcome::Ok {
                payload: ResponsePayload::TaskState {
                    projects: Vec::new(),
                    sessions: vec![session; 5001],
                    default_cwd: PathBuf::from("/fixture/space λ"),
                    projectless_root: Some(PathBuf::from("/fixture/scratch")),
                },
            },
        };
        let text = serde_json::to_string(&message).unwrap();
        assert!(catalog_response(&text).is_some());
        check_parity(&text);
        let legacy = response(
            r#"{"sessions":[],"projects":[],"defaultCwd":"/fixture","type":"taskState","unknown":{"nested":[1,2]}}"#,
        );
        assert!(catalog_response(&legacy).is_some());
        check_parity(&legacy);
        check_parity(&legacy.replace("taskState", r"task\u0053tate"));
        check_parity(&legacy.replace("response", r"respon\u0073e"));
    }

    #[test]
    fn catalog_decoder_preserves_rejections_and_other_messages() {
        for payload in [
            r#"{"type":"taskState","projects":[],"sessions":[],"defaultCwd":"/fixture","projectlessRoot":null}"#,
            r#"{"type":"taskState","projects":[],"sessions":[]}"#,
            r#"{"type":"taskState","projects":[],"sessions":null,"defaultCwd":"/fixture"}"#,
            r#"{"type":"taskState","projects":[],"sessions":[{}],"defaultCwd":"/fixture"}"#,
            r#"{"type":"taskState","type":"taskState","projects":[],"sessions":[],"defaultCwd":"/fixture"}"#,
            r#"{"type":"taskState","projects":[],"sessions":[],"sessions":[],"defaultCwd":"/fixture"}"#,
            r#"{"type":"ack"}"#,
            r#"{"type":"unknown"}"#,
        ] {
            check_parity(&response(payload));
        }
        let valid =
            response(r#"{"type":"taskState","projects":[],"sessions":[],"defaultCwd":"/fixture"}"#);
        for text in [
            valid.replace("\"type\":\"response\"", "\"type\":\"response\",\"type\":\"response\""),
            valid.replace("\"status\":\"ok\"", "\"status\":\"ok\",\"status\":\"ok\""),
            format!("{valid} trailing"),
            r#"{"type":"hello","protocolVersion":20,"daemonVersion":"fixture","daemonCommit":null}"#.into(),
            r#"{"type":"taskStateChanged","revision":3}"#.into(),
            r#"{"type":"rejected","message":"fixture"}"#.into(),
        ] {
            check_parity(&text);
        }
    }
}
