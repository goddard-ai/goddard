//! Turn status markers: while the experiment is on, each assistant turn that
//! ends naturally is scored by the daemon's evaluation model against a fixed
//! marker set — the common good outcomes plus the rarer bad states a settled
//! turn can hide (blocked, drifted, stopped early). A marker whose
//! probability clears its threshold renders as a chip on the response
//! footer: colored icon and name, dim confidence percent.
//!
//! Evals are spend, so they only run for the session on screen. A turn that
//! ends while its session is unselected queues behind it and is scored when
//! that session is next opened. The backend call and the decision-log write
//! stay daemon-side behind `Command::Evaluate`; this file is the app-side
//! seam, the same shape `routing.rs` takes.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use uuid::Uuid;
use waku_protocol::eval::{EvalAnswer, EvalQuestion, Evaluation};
use waku_protocol::model::{ActivityKind, AgentSession, MessageRole};

use super::*;

/// One status the evaluation model scores each settled turn against. `id` is
/// the question key and the decision-log marker name; `threshold` is the
/// probability a `Noul` answer must reach before the marker renders.
struct StatusMarker {
    id: &'static str,
    label_key: &'static str,
    icon: &'static str,
    tone: MarkerTone,
    threshold: f64,
    instructions: &'static str,
}

/// Which theme color a marker's icon and label take.
enum MarkerTone {
    Success,
    Info,
    Warning,
    Danger,
    Favorite,
}

impl MarkerTone {
    fn color(&self, theme: &Theme) -> Hsla {
        match self {
            Self::Success => theme.success,
            Self::Info => theme.info,
            Self::Warning => theme.warning,
            Self::Danger => theme.danger,
            Self::Favorite => theme.favorite,
        }
    }
}

/// The fixed marker set. Bad states carry lower thresholds — a false chip is
/// cheap next to a missed failure — while `complete` only renders when the
/// model is sure.
const STATUS_MARKERS: &[StatusMarker] = &[
    StatusMarker {
        id: "complete",
        label_key: "status_markers.complete",
        icon: "icons/check.svg",
        tone: MarkerTone::Success,
        threshold: 0.80,
        instructions: "Did the assistant fully address the user's request for this turn? \
            Answer true only when the requested work was carried out end to end and the \
            final response reports it done.",
    },
    StatusMarker {
        id: "awaiting-input",
        label_key: "status_markers.awaiting_input",
        icon: "icons/chat.svg",
        tone: MarkerTone::Info,
        threshold: 0.70,
        instructions: "Does this turn end waiting on the user — an unanswered question, a \
            choice the user must pick, or input the assistant needs before it can \
            continue?",
    },
    StatusMarker {
        id: "unverified",
        label_key: "status_markers.unverified",
        icon: "icons/eye.svg",
        tone: MarkerTone::Warning,
        threshold: 0.75,
        instructions: "Did the assistant finish without verifying its work — code it \
            changed but did not build, test, or run, or claims it did not check?",
    },
    StatusMarker {
        id: "partial",
        label_key: "status_markers.partial",
        icon: "icons/hourglass.svg",
        tone: MarkerTone::Warning,
        threshold: 0.65,
        instructions: "Did the turn stop before the work was finished — cut off by a \
            context or step limit, truncated output, or an explicit promise to continue \
            later?",
    },
    StatusMarker {
        id: "blocked",
        label_key: "status_markers.blocked",
        icon: "icons/block.svg",
        tone: MarkerTone::Danger,
        threshold: 0.60,
        instructions: "Is the assistant unable to proceed without something outside its \
            control — a missing permission, credential, file, tool, or environment \
            resource?",
    },
    StatusMarker {
        id: "failed",
        label_key: "status_markers.failed",
        icon: "icons/x.svg",
        tone: MarkerTone::Danger,
        threshold: 0.60,
        instructions: "Did the turn fail — a reported error, a tool failure that ended \
            the work, or the assistant saying it could not complete the request?",
    },
    StatusMarker {
        id: "drifted",
        label_key: "status_markers.drifted",
        icon: "icons/target.svg",
        tone: MarkerTone::Favorite,
        threshold: 0.70,
        instructions: "Did the assistant do work outside the scope of what the user \
            asked — unrelated edits, a different task, or unrequested scope creep?",
    },
];

/// Response text sent to the evaluator is capped so a long final message
/// cannot blow up the request — the marker judgments all read the tail.
const RESPONSE_STATE_CHARS: usize = 6_000;
/// Failed tool calls summarized for the evaluator, most recent last.
const TOOL_ERROR_STATE_MAX: usize = 10;
/// Changed paths listed for the evaluator; a checkpoint longer than this is
/// summarized by count instead.
const FILES_CHANGED_STATE_MAX: usize = 20;

/// The decision-log feature tag these evaluations record under, so the
/// calibration dataset keeps them distinct from ad-hoc `evaluate` calls.
const EVAL_FEATURE: &str = "turn-status";

/// The question map sent with every turn evaluation: one `Noul` per marker
/// so states can co-surface instead of competing for a single choice.
fn status_marker_questions() -> BTreeMap<String, EvalQuestion> {
    STATUS_MARKERS
        .iter()
        .map(|marker| {
            (
                marker.id.to_owned(),
                EvalQuestion::Noul {
                    instructions: marker.instructions.to_owned(),
                    criteria: None,
                },
            )
        })
        .collect()
}

/// The `state` the evaluation judges: the prompt that opened the turn, the
/// assistant's closing text, tool-call and error counts, the changed-file
/// list when the checkpoint has landed, and the provider's own turn summary.
/// Each marker's instructions read whichever fields its judgment needs.
fn turn_eval_state(session: &AgentSession, turn_id: Uuid, summary: Option<&str>) -> Value {
    let prompt = session
        .messages
        .iter()
        .find(|message| message.turn_id == Some(turn_id) && message.role == MessageRole::User)
        .map(|message| message.visible_content().to_owned());
    let mut response = session
        .messages
        .iter()
        .filter(|message| {
            message.turn_id == Some(turn_id) && message.role == MessageRole::Assistant
        })
        .map(|message| message.visible_content())
        .collect::<Vec<_>>()
        .join("\n\n");
    if response.chars().count() > RESPONSE_STATE_CHARS {
        response = response
            .chars()
            .skip(response.chars().count() - RESPONSE_STATE_CHARS)
            .collect();
    }
    let activities: Vec<&ActivityItem> = session
        .transcript_blocks
        .iter()
        .filter(|block| block.turn_id == Some(turn_id))
        .flat_map(|block| block.activities.iter())
        .collect();
    let tool_errors: Vec<String> = activities
        .iter()
        .filter(|activity| activity.failed)
        .map(|activity| match &activity.tool_name {
            Some(tool) => format!("{} ({tool})", activity.title),
            None => activity.title.clone(),
        })
        .take(TOOL_ERROR_STATE_MAX)
        .collect();
    let turn = session.turns.iter().find(|turn| turn.id == turn_id);
    let files_changed: Vec<String> = turn
        .and_then(|turn| turn.checkpoint.as_ref())
        .map(|checkpoint| {
            checkpoint
                .files
                .iter()
                .take(FILES_CHANGED_STATE_MAX)
                .map(|file| file.path.clone())
                .collect()
        })
        .unwrap_or_default();
    json!({
        "prompt": prompt,
        "response": response,
        "finish": { "success": true, "summary": summary },
        "provider": session.provider.display_name(),
        "model": session.model,
        "toolCalls": activities
            .iter()
            .filter(|activity| activity.kind != ActivityKind::Reasoning)
            .count(),
        "toolErrors": tool_errors,
        "filesChanged": files_changed,
    })
}

/// The markers an evaluation cleared, in catalog order — what the footer row
/// renders.
fn cleared_markers(evaluation: &Evaluation) -> Vec<(&'static StatusMarker, f64)> {
    STATUS_MARKERS
        .iter()
        .filter_map(|marker| {
            let noul = match evaluation.answers.get(marker.id) {
                Some(EvalAnswer::Noul { noul }) => *noul,
                _ => return None,
            };
            (noul >= marker.threshold).then_some((marker, noul))
        })
        .collect()
}

/// One footer chip: the marker's colored icon and name plus the dim
/// confidence the model reported.
#[track_caller]
fn status_marker_chip(marker: &StatusMarker, noul: f64, theme: &Theme) -> Div {
    let color = marker.tone.color(theme);
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(icon(marker.icon, 11.0, color))
        .child(
            div()
                .text_size(sp(11.0))
                .text_color(color)
                .child(tr!(marker.label_key)),
        )
        .child(
            div()
                .text_size(sp(11.0))
                .text_color(theme.text_ghost)
                .child(format!("{}%", (noul * 100.0).round() as u32)),
        )
}

impl Waku {
    /// Route a settled turn into evaluation: score it now when its session is
    /// on screen, queue it behind the session otherwise — the next open
    /// drains the queue. Failed or interrupted turns never reach here; a
    /// natural end is the only state worth judging.
    pub(super) fn note_turn_finished_for_status_markers(
        &mut self,
        session_id: Uuid,
        turn_id: Option<Uuid>,
        summary: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(turn_id) = turn_id else {
            return;
        };
        if !self.state.status_markers_enabled {
            return;
        }
        if self.state.selected_session == Some(session_id) {
            self.request_status_marker_eval(session_id, turn_id, summary, cx);
        } else {
            self.pending_status_marker_turns
                .entry(session_id)
                .or_default()
                .push((turn_id, summary));
        }
    }

    /// Evaluate every turn that settled while `session_id` was off screen.
    /// Called when the session becomes the selected one.
    pub(super) fn drain_pending_status_marker_turns(
        &mut self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_status_marker_turns.remove(&session_id) else {
            return;
        };
        for (turn_id, summary) in pending {
            self.request_status_marker_eval(session_id, turn_id, summary, cx);
        }
    }

    /// Build the turn's state and ask its session's daemon for the marker
    /// scores, off the UI thread. An unconfigured eval backend on that daemon
    /// means the feature simply does not fire — no row, no error.
    fn request_status_marker_eval(
        &mut self,
        session_id: Uuid,
        turn_id: Uuid,
        summary: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.state.status_markers_enabled || self.status_marker_in_flight.contains(&turn_id) {
            return;
        }
        let Some(daemon) = self.daemons.daemon_for_session(session_id) else {
            return;
        };
        if daemon.settings().eval.is_none() {
            return;
        }
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        let state = turn_eval_state(session, turn_id, summary.as_deref());
        let questions = status_marker_questions();
        let tx = self.status_marker_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        self.status_marker_in_flight.insert(turn_id);
        cx.background_executor()
            .spawn(async move {
                let result = daemon
                    .client()
                    .request(
                        Uuid::nil(),
                        session_id,
                        waku_client::Command::Evaluate {
                            state,
                            questions,
                            feature: Some(EVAL_FEATURE.to_owned()),
                            timeout_secs: None,
                        },
                    )
                    .map_err(|error| format!("{error:#}"))
                    .and_then(|payload| match payload {
                        waku_client::ResponsePayload::Evaluation { evaluation } => Ok(evaluation),
                        _ => Err("the daemon returned an invalid evaluation response".to_owned()),
                    });
                if tx.send((turn_id, result)).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .detach();
    }

    /// Land answered evaluations. A failed call drops quietly — the turn's
    /// footer simply never gains chips, matching every other eval fallback.
    pub(super) fn drain_status_marker_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok((turn_id, result)) = self.status_marker_events.try_recv() {
            self.status_marker_in_flight.remove(&turn_id);
            match result {
                Ok(evaluation) => {
                    self.turn_status_markers.insert(turn_id, evaluation);
                    changed = true;
                }
                Err(error) => {
                    eprintln!("Goddard: status marker evaluation failed: {error}");
                }
            }
        }
        changed
    }

    /// The marker chips a settled turn's response footer shows, if the
    /// evaluation answered and at least one marker cleared its threshold.
    #[track_caller]
    pub(super) fn render_status_marker_row(
        &self,
        turn_id: Uuid,
        theme: &Theme,
    ) -> Option<AnyElement> {
        let cleared = cleared_markers(self.turn_status_markers.get(&turn_id)?);
        if cleared.is_empty() {
            return None;
        }
        Some(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(12.0))
                .children(
                    cleared
                        .into_iter()
                        .map(|(marker, noul)| status_marker_chip(marker, noul, theme)),
                )
                .into_any_element(),
        )
    }

    /// Forget every marker verdict and pending request; the feature's state
    /// is runtime-only by design, so toggling off wipes it rather than
    /// leaving stale chips on screen.
    pub(super) fn clear_status_markers(&mut self) {
        self.turn_status_markers.clear();
        self.pending_status_marker_turns.clear();
        self.status_marker_in_flight.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluation(answers: BTreeMap<String, EvalAnswer>) -> Evaluation {
        Evaluation {
            model: "jev-test".to_owned(),
            answers,
            usage: Default::default(),
            latency_ms: 0,
            provider_metadata: None,
        }
    }

    #[test]
    fn questions_cover_every_marker_as_noul() {
        let questions = status_marker_questions();
        assert_eq!(questions.len(), STATUS_MARKERS.len());
        for marker in STATUS_MARKERS {
            assert!(matches!(
                questions.get(marker.id),
                Some(EvalQuestion::Noul { .. })
            ));
        }
    }

    #[test]
    fn cleared_markers_respect_thresholds() {
        let evaluation = evaluation(
            [
                ("complete", 0.9),
                ("awaiting-input", 0.69),
                ("blocked", 0.61),
                ("failed", 0.59),
            ]
            .into_iter()
            .map(|(id, noul)| (id.to_owned(), EvalAnswer::Noul { noul }))
            .collect(),
        );
        let cleared = cleared_markers(&evaluation);
        let ids: Vec<&str> = cleared.iter().map(|(marker, _)| marker.id).collect();
        // 0.69 misses awaiting-input's 0.70; 0.61 clears blocked's 0.60 while
        // 0.59 misses failed's — the per-marker threshold is what decides.
        assert_eq!(ids, ["complete", "blocked"]);
    }

    #[test]
    fn turn_state_carries_prompt_response_and_turn_facts() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let turn_id = session.begin_turn("Fix the bug");
        session.push_message(MessageRole::Assistant, "Fixed it.");
        let state = turn_eval_state(&session, turn_id, Some("wrap-up"));
        assert_eq!(state["prompt"], "Fix the bug");
        assert_eq!(state["response"], "Fixed it.");
        assert_eq!(state["finish"]["summary"], "wrap-up");
        assert_eq!(state["provider"], "Codex CLI");
    }
}
