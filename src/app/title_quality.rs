//! One Jev judgment after each successful turn, then an isolated provider
//! request to rewrite only automatic titles that are clearly confusing or noisy.

use std::collections::BTreeMap;

use serde_json::json;
use uuid::Uuid;
use waku_protocol::eval::{EvalAnswer, EvalQuestion};
use waku_protocol::git::{AgentInvocation, CLAUDE_COMMIT_MODEL, CODEX_COMMIT_MODEL};

use super::*;

const FEATURE: &str = "title-quality";
const QUESTION: &str = "needs-rewrite";
const REWRITE_THRESHOLD: f64 = 0.8;

impl Waku {
    pub(super) fn check_session_title_quality(
        &mut self,
        session_id: Uuid,
        turn_id: Option<Uuid>,
        summary: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(turn_id) = turn_id else { return };
        let Some(daemon) = self.daemon_for_session(session_id) else {
            return;
        };
        if daemon
            .settings()
            .eval
            .as_ref()
            .is_none_or(|eval| eval.credential_missing())
        {
            return;
        }
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .filter(|session| session.title == AgentSession::DEFAULT_TITLE)
        else {
            return;
        };
        let title = session.display_title().to_owned();
        if self.title_quality_in_flight.contains(&session_id) {
            return;
        }
        let provider = session.provider;
        // These one-shot clients do not expose a model selector, so a title
        // rewrite could silently run on an expensive default.
        if matches!(provider, ProviderKind::DeepSeek | ProviderKind::Fx) {
            return;
        }
        let model = self
            .daemon_settings_for_session(session_id)
            .title_models
            .get(&provider)
            .cloned()
            .or_else(|| match provider {
                ProviderKind::Claude => Some(CLAUDE_COMMIT_MODEL.to_owned()),
                ProviderKind::Codex => Some(CODEX_COMMIT_MODEL.to_owned()),
                _ => None,
            });
        let Some(model) = model else { return };
        let Some(binary) = self.provider_binary_for_session(session_id, provider) else {
            return;
        };
        let Some(cwd) = self
            .workspace_path_for_session(session)
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        let Some(client) = self.workspace_client_for_path(&cwd) else {
            return;
        };
        let mut state = status_markers::turn_eval_state(session, turn_id, summary.as_deref());
        state["title"] = json!(title);
        let user_request = state["prompt"]
            .as_str()
            .into_iter()
            .chain(
                state["priorPrompts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str),
            )
            .collect::<Vec<_>>()
            .join("\n\n");
        let completion = state["response"].as_str().unwrap_or_default().to_owned();
        let questions = BTreeMap::from([(
            QUESTION.to_owned(),
            EvalQuestion::Noul {
                instructions: "Is the current task title confusing, misleading, generic, or noisy for the user's request and completed work? Answer true only when a clearer short title would materially help identify the task. A specific, readable title should remain unchanged.".to_owned(),
                criteria: None,
            },
        )]);
        let tx = self.title_quality_tx.clone();
        let wake = self.event_wake_tx.clone();
        self.title_quality_in_flight.insert(session_id);
        cx.background_executor()
            .spawn(async move {
                let rewrite = daemon
                    .client()
                    .request(
                        Uuid::nil(),
                        session_id,
                        waku_client::Command::Evaluate {
                            state,
                            questions,
                            feature: Some(FEATURE.to_owned()),
                            timeout_secs: None,
                        },
                    )
                    .ok()
                    .and_then(|result| match result {
                        waku_client::ResponsePayload::Evaluation { evaluation } => evaluation
                            .answers
                            .get(QUESTION)
                            .and_then(|answer| match answer {
                                EvalAnswer::Noul { noul } => Some(*noul >= REWRITE_THRESHOLD),
                                _ => None,
                            }),
                        _ => None,
                    })
                    .unwrap_or(false);
                let next_title = if rewrite {
                    client
                        .request(waku_client::WorkspaceOperation::GenerateSessionTitle {
                            cwd,
                            current_title: title.clone(),
                            user_request,
                            completion,
                            invocation: AgentInvocation {
                                provider,
                                binary,
                                model: Some(model),
                                reasoning_effort: None,
                            },
                        })
                        .ok()
                        .and_then(|result| match result {
                            waku_client::WorkspaceResult::SessionTitle { title } => Some(title),
                            _ => None,
                        })
                } else {
                    None
                };
                if tx.send((session_id, title, next_title)).is_ok() {
                    signal_event_pump(&wake);
                }
            })
            .detach();
    }

    pub(super) fn drain_title_quality_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok((session_id, old_title, next_title)) = self.title_quality_events.try_recv() {
            self.title_quality_in_flight.remove(&session_id);
            if let Some(next_title) = next_title
                && let Some(session) = self.state.session_mut(session_id)
                && session.title == AgentSession::DEFAULT_TITLE
                && session.display_title() == old_title
            {
                changed |= session.set_auto_title(Some(next_title));
            } else if let Some(turn_id) = self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .filter(|session| {
                    session.title == AgentSession::DEFAULT_TITLE
                        && session.display_title() != old_title
                })
                .and_then(|session| {
                    session
                        .turns
                        .iter()
                        .rev()
                        .find(|turn| turn.status == TurnStatus::Completed)
                        .map(|turn| turn.id)
                })
            {
                self.check_session_title_quality(session_id, Some(turn_id), None, cx);
            }
        }
        if changed {
            self.save();
        }
        changed
    }
}
