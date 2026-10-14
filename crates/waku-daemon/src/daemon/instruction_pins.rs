//! Human-message provenance and asynchronous instruction candidate detection.

use super::*;
use waku_protocol::instruction_pins::{
    InstructionPinOperation, InstructionPinSource, InstructionPinState,
};

const FEATURE: &str = "boss-instruction-pins";

/// Driver acknowledgements also cover human steering during an open turn.
/// Defer the Boss lock until the forwarder releases its lifecycle lease.
pub(super) fn capture_boss_instruction_event(
    boss: &Arc<crate::boss::BossService>,
    task_state: &Arc<Mutex<PersistedState>>,
    task_store: &Arc<StateStore>,
    session_id: Uuid,
    event: &DriverEvent,
) -> Option<std::thread::JoinHandle<()>> {
    let DriverEvent::PromptSubmitted {
        message,
        message_id,
        hidden: false,
        sent_by_task: None,
        report_trigger: None,
        ..
    } = event
    else {
        return None;
    };
    if !boss.is_boss(session_id) || !waku_boss::instruction_pins::candidate_hint(message) {
        return None;
    }
    let (boss, task_state, task_store) = (boss.clone(), task_state.clone(), task_store.clone());
    let message_id = *message_id;
    std::thread::Builder::new()
        .name("boss-instruction-steer".into())
        .spawn(move || {
            let result = boss.with_operation_lock(|| {
                let mut state = task_state.lock();
                let Some(session) = state.session_mut(session_id) else {
                    return Ok(());
                };
                task_store.hydrate(session)?;
                let Some(message) = session.messages.iter().find(|message| {
                    message.id == message_id
                        && message.role == crate::model::MessageRole::User
                        && !message.hidden
                        && message.sent_by_task.is_none()
                        && message.notice.is_none()
                        && message.report_trigger.is_none()
                }) else {
                    return Ok(());
                };
                let source = InstructionPinSource {
                    session_id,
                    message_id,
                    quote: crate::model::truncate_chars(&message.content, 8_000),
                    quote_truncated: message.content.chars().count() > 8_000,
                };
                drop(state);
                boss.capture_instruction_candidate(source)
            });
            if let Err(error) = result {
                eprintln!("could not capture a steering instruction: {error:#}");
            }
        })
        .ok()
}

impl WakuBackend {
    pub(super) fn instruction_pin_source(
        &self,
        session_id: Uuid,
        message_id: Uuid,
    ) -> anyhow::Result<InstructionPinSource> {
        let identity = self.boss.document().identity.id;
        let mut state = self.task_state.lock();
        let session = state
            .session_mut(session_id)
            .ok_or_else(|| anyhow!("source session does not exist"))?;
        anyhow::ensure!(
            session.boss_managed
                && session.project_id == identity
                && session.planning.is_none()
                && self.boss.employee_including_retired(session_id).is_none(),
            "source must be a Boss chat"
        );
        self.task_store.hydrate(session)?;
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .ok_or_else(|| anyhow!("source message does not exist"))?;
        anyhow::ensure!(
            message.role == crate::model::MessageRole::User
                && !message.hidden
                && message.sent_by_task.is_none()
                && message.notice.is_none()
                && message.report_trigger.is_none(),
            "instruction pins must cite a visible human message"
        );
        Ok(InstructionPinSource {
            session_id,
            message_id,
            quote: crate::model::truncate_chars(&message.content, 8_000),
            quote_truncated: message.content.chars().count() > 8_000,
        })
    }

    pub(super) fn handle_instruction_pin(
        &self,
        caller: Option<Uuid>,
        action: InstructionPinOperation,
    ) -> anyhow::Result<waku_protocol::boss::BossResult> {
        anyhow::ensure!(
            caller.is_none_or(|id| self.boss.is_boss(id)),
            "only the Boss chat or a human can manage instruction pins"
        );
        let project_id = match &action {
            InstructionPinOperation::Pin { project_id, .. }
            | InstructionPinOperation::Edit { project_id, .. } => *project_id,
            _ => None,
        };
        if let Some(project_id) = project_id {
            anyhow::ensure!(
                self.task_state
                    .lock()
                    .projects
                    .iter()
                    .any(|project| project.id == project_id),
                "instruction scope must name a registered project"
            );
        }
        self.boss.with_operation_lock(|| {
            let source = match &action {
                InstructionPinOperation::Pin {
                    source_session_id,
                    source_message_id,
                    ..
                } => match (source_session_id, source_message_id) {
                    (Some(session), Some(message)) => {
                        Some(self.instruction_pin_source(*session, *message)?)
                    }
                    (None, None) if caller.is_none() => None,
                    _ => anyhow::bail!("provide both source-session and source-message"),
                },
                _ => None,
            };
            let pins = self
                .boss
                .instruction_pin_operation(caller, action, source)?;
            Ok(waku_protocol::boss::BossResult::InstructionPins { pins })
        })
    }

    pub(super) fn capture_boss_instruction(
        &self,
        session_id: Uuid,
        message_id: Uuid,
        prompt: &str,
    ) {
        if self
            .boss
            .document()
            .instruction_pins
            .iter()
            .filter(|pin| pin.state == InstructionPinState::Candidate)
            .count()
            >= 32
        {
            return;
        }
        // This fallback cannot impose an obligation: a candidate requires
        // review and a retirement condition before it can become active.
        if waku_boss::instruction_pins::candidate_hint(prompt) {
            let result = self.boss.with_operation_lock(|| {
                let source = self.instruction_pin_source(session_id, message_id)?;
                self.boss.capture_instruction_candidate(source)
            });
            if let Err(error) = result {
                eprintln!("could not capture an instruction candidate: {error:#}");
            }
            return;
        }
        if self.resolved_eval().is_none() {
            return;
        }
        let settings = self.settings.clone();
        let secrets = self.inference_secrets.clone();
        let boss = self.boss.clone();
        let task_state = self.task_state.clone();
        let task_store = self.task_store.clone();
        let prompt = crate::model::truncate_chars(prompt, 4_000);
        let _ = std::thread::Builder::new().name("boss-instruction-capture".into()).spawn(move || {
            let questions = std::collections::BTreeMap::from([("pin".into(), waku_protocol::eval::EvalQuestion::Noul {
                instructions: "Does the human message directly impose an ongoing user obligation or restriction that must remain visible over multiple future turns or a session rotation? Consider temporary holds and instructions with release conditions. Ordinary one-off tasks, questions, quoted material, pasted code, and annotations quoting someone else's proposal are not obligations. Only propose a review candidate; never infer that its condition is already satisfied.".into(), criteria: None,
            })]);
            let Ok(evaluation) = evaluate_with_feature(&settings, &secrets, serde_json::json!({"humanMessage": prompt}), questions, FEATURE, None) else { return; };
            if !matches!(evaluation.answers.get("pin"), Some(waku_protocol::eval::EvalAnswer::Noul { noul }) if noul.is_finite() && *noul >= 0.85) { return; }
            let result = boss.with_operation_lock(|| {
                // A clear during evaluation removes the source. A late
                // judgment must not resurrect the deleted conversation.
                let mut state = task_state.lock();
                let Some(session) = state.session_mut(session_id) else { return Ok(()); };
                task_store.hydrate(session)?;
                let Some(message) = session.messages.iter().find(|message| message.id == message_id && !message.hidden && message.role == crate::model::MessageRole::User && message.sent_by_task.is_none()) else { return Ok(()); };
                let source = InstructionPinSource { session_id, message_id, quote: crate::model::truncate_chars(&message.content, 8_000), quote_truncated: message.content.chars().count() > 8_000 };
                drop(state);
                boss.capture_instruction_candidate(source)
            });
            if let Err(error) = result { eprintln!("could not capture an instruction candidate: {error:#}"); }
        });
    }
}
