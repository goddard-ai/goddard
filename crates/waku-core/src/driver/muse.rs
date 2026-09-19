//! Muse Code via MSP: every session rides the shared `muse serve` host.
//!
//! The host process and the session fan-out live in [`crate::muse_service`];
//! this file is one session's worker plus its `DriverControl` surface.
//!
//! Lifecycle: Goddard mints the `sessionId` (UUIDv7) itself and subscribes
//! BEFORE `session/start`, because the host auto-subscribes this connection
//! and may emit the session's first event before the response lands. Resume
//! goes through `session/resume`, whose pending approval/user-input pointers
//! arrive right after as re-issued server requests — the service acks them
//! and republishes, so they flow through the same handlers as live requests.
//!
//! Turn lifecycle is command-then-event: `turn/start` is admission only, and
//! the outcome arrives as `turn/completed`. `TurnStarted` follows the ack's
//! `disposition` — a queued submission starts when `turn/started` arrives.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, bail};
use base64::Engine as _;
use crossbeam_channel::{Sender, bounded, unbounded};
use serde_json::{Value, json};

use super::activity;
use crate::driver::{DriverControl, DriverEventSender, DriverStartOptions, SessionOptions};
use crate::model::{
    ActivityItem, ActivityKind, DriverEvent, MessageAttachment, PermissionOption,
    ProviderResumeCursor, RuntimeMode, ThreadGoal, ThreadGoalStatus, UserInputAnswer,
    UserInputOption, UserInputQuestion,
};
use crate::muse_service::{self, MuseError, MuseFrame, MuseService, MuseSubscription};
use crate::muse_session::{FinishedTurn, finished_turns, fork_boundary_id};

/// Answer paths for `DriverControl::fork` / `rollback`, which block the
/// daemon's request thread until the host replies.
const ACTION_TIMEOUT: Duration = Duration::from_secs(30);
/// Largest provider payload kept on an activity card.
const MAX_DETAIL_CHARS: usize = 16_000;

enum DriverCommand {
    Prompt {
        text: String,
        attachments: Vec<MessageAttachment>,
    },
    Steer(String),
    Cancel,
    Respond {
        request_id: String,
        option_id: String,
    },
    RespondUserInput {
        request_id: String,
        answers: Vec<UserInputAnswer>,
    },
    ClarifyUserInput {
        request_id: String,
        content: String,
    },
    CancelUserInput {
        request_id: String,
    },
    ApplyOptions(SessionOptions, Sender<bool>),
    /// Fork only: the cursor names the new session; this driver stays put.
    Fork {
        turns: usize,
        reply: Sender<anyhow::Result<ProviderResumeCursor>>,
    },
    /// Goddard's rewind is fork-then-attach: the driver moves to the fork.
    Rollback {
        turns: usize,
        reply: Sender<anyhow::Result<ProviderResumeCursor>>,
    },
    Shutdown,
}

/// The answer channel for an open `approval/requested` / `approval/request`.
struct PendingApproval {
    requirement_id: Value,
}

/// The questions of an open `userInput/requested`, kept so a text-only
/// `UserInputAnswer` round-trips back into the right wire shape.
struct PendingUserInput {
    questions: Vec<Value>,
}

struct ItemState {
    kind: String,
    /// Text already emitted as `TextDelta`/`ReasoningDelta`.
    streamed: usize,
}

struct WorkerState {
    session_id: String,
    mode: RuntimeMode,
    /// The evaluation backend answering `Auto`-mode approval requests,
    /// snapshotted at session start.
    eval: Option<Arc<waku_protocol::eval::EvalSettings>>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    active_turn: Option<String>,
    /// Finished turns in order — every `turn/completed`, whatever its
    /// terminal, so the list lines up with Goddard's provider-turn count.
    /// Only `completed` entries are legal `session/fork` boundaries.
    finished_turns: Vec<FinishedTurn>,
    /// Admitted-but-not-launched submits, in launch order. `turn/interrupt`
    /// stops only the foreground turn, so Cancel must reclaim each of these
    /// with `turn/unqueue` before they fire `turn/started` and take over.
    queued_turns: Vec<String>,
    /// Turns with an open retry card, completed when the turn ends.
    retried_turns: HashSet<String>,
    items: HashMap<String, ItemState>,
    approvals: HashMap<String, PendingApproval>,
    user_inputs: HashMap<String, PendingUserInput>,
    subscription: MuseSubscription,
}

struct Worker {
    service: MuseService,
    events: DriverEventSender,
}

pub(super) struct MuseDriver {
    // Kept so the pool sees one live handle until drop.
    #[allow(dead_code)]
    service: MuseService,
    commands: Sender<DriverCommand>,
}

impl MuseDriver {
    /// Runs on the daemon request thread; blocking is expected here.
    pub(super) fn start(
        options: DriverStartOptions,
        events: DriverEventSender,
    ) -> anyhow::Result<Self> {
        let DriverStartOptions {
            binary,
            cwd,
            mode,
            model,
            reasoning_effort,
            service_tier: _,
            context_window: _,
            agent_preset: _,
            computer_use_enabled: _,
            agent: _,
            subagents: _,
            provider_cursor,
            eval,
        } = options;

        let (resumed_id, resume_cursor) = match provider_cursor {
            Some(ProviderResumeCursor::Muse {
                session_id,
                view_cursor,
            }) if !session_id.is_empty() => (Some(session_id), view_cursor),
            Some(cursor) => {
                return Err(anyhow!(
                    "cannot resume Muse Code from a {} cursor",
                    cursor.provider().display_name()
                ));
            }
            _ => (None, None),
        };

        let service = muse_service::acquire(&binary)?;
        // Goddard mints the id (UUIDv7 is the host's session-id shape) so the
        // subscription can exist before `session/start` returns.
        let resuming = resumed_id.is_some();
        let session_id = resumed_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let subscription = service.subscribe(&session_id);
        let workspace_root = cwd.to_string_lossy().into_owned();

        let result = if resuming {
            let mut params = json!({
                "commandId": service.mint_command_id(),
                "sessionId": session_id,
            });
            if let Some(cursor) = resume_cursor.as_deref() {
                params["cursor"] = json!(cursor);
            } else {
                // A known cursor resumes suffix-only; without one ask for the
                // folded snapshot so usage/mode state survives the reattach
                // without replaying items the daemon already stored.
                params["history"] = json!("snapshot");
            }
            service.call("session/resume", params)
        } else {
            let mut params = json!({
                "commandId": service.mint_command_id(),
                "sessionId": session_id,
                "workspaceRoot": workspace_root,
                "approvalMode": approval_mode(mode),
            });
            if let Some(model) = model.as_deref() {
                params["modelId"] = json!(model);
            }
            service.call("session/start", params)
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => return Err(muse_error("could not open a Muse Code session", &error)),
        };
        let session = result.get("session").cloned().unwrap_or(Value::Null);

        let (commands, command_rx) = unbounded();
        let worker = Worker {
            service: service.clone(),
            events,
        };
        let mut state = WorkerState {
            session_id: session_id.clone(),
            mode,
            eval: eval.map(Arc::new),
            model,
            reasoning_effort,
            active_turn: session
                .get("activeTurnId")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    session
                        .get("activeTurn")
                        .and_then(|turn| turn.get("turnId"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                }),
            // The resume result carries no finished-turn list — items have
            // no terminal — so rebuild it from the view. A cursor resume's
            // suffix replay repopulates it through `turn/completed` events
            // anyway; this makes snapshot resumes forkable too.
            finished_turns: if resuming {
                finished_turns(&service, &session_id).unwrap_or_default()
            } else {
                Vec::new()
            },
            queued_turns: Vec::new(),
            retried_turns: HashSet::new(),
            items: HashMap::new(),
            approvals: HashMap::new(),
            user_inputs: HashMap::new(),
            subscription,
        };
        restore_snapshot(&result, &mut state, &worker.events);

        let _ = worker.events.send(DriverEvent::Connected {
            provider_cursor: Some(ProviderResumeCursor::Muse {
                session_id: session_id.clone(),
                view_cursor: result
                    .get("viewCursor")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            }),
        });
        if state.active_turn.is_some() {
            let _ = worker.events.send(DriverEvent::TurnStarted);
        }

        thread::Builder::new()
            .name(format!("waku-muse-{session_id}"))
            .spawn(move || {
                loop {
                    // Cloned each iteration: a rollback swaps the subscription
                    // and the loop must follow it to the new channel.
                    let frames = state.subscription.rx.clone();
                    crossbeam_channel::select! {
                        recv(command_rx) -> message => {
                            let Ok(message) = message else { return };
                            if !handle_command(&worker, message, &mut state) {
                                return;
                            }
                        }
                        recv(frames) -> frame => {
                            let Ok(frame) = frame else { return };
                            match frame {
                                MuseFrame::Event { method, params } => {
                                    handle_event(
                                        Some(&worker.service),
                                        &worker.events,
                                        &mut state,
                                        &method,
                                        &params,
                                    );
                                }
                                // A Goddard-owned host does not reconnect: a
                                // dead process is the end of every session it
                                // served, and the daemon surfaces the exit.
                                MuseFrame::Exited => {
                                    let _ = worker.events.send(DriverEvent::ProcessExited);
                                    return;
                                }
                            }
                        }
                    }
                }
            })?;

        Ok(Self { service, commands })
    }
}

fn restore_snapshot(result: &Value, state: &mut WorkerState, events: &DriverEventSender) {
    let Some(snapshot) = result.pointer("/history/snapshot/state") else {
        return;
    };
    if let Some(usage) = snapshot.get("contextUsage") {
        let context_tokens = usage.get("usedTokens").and_then(Value::as_u64);
        let context_window = usage.get("windowTokens").and_then(Value::as_u64);
        if context_tokens.is_some() || context_window.is_some() {
            let _ = events.send(DriverEvent::UsageUpdated {
                context_tokens,
                context_window,
            });
        }
    }
    if state.active_turn.is_none()
        && let Some(turn) = snapshot.get("activeTurn")
        && let Some(turn_id) = turn.get("turnId").and_then(Value::as_str)
    {
        state.active_turn = Some(turn_id.to_owned());
    }
    if let Some(queued) = snapshot.get("queuedTurns").and_then(Value::as_array) {
        state.queued_turns = queued
            .iter()
            .filter_map(|turn| {
                turn.get("turnId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
    }
}

/// Record a finished turn once, in view order; a later `turn/completed`
/// upgrades a retracted entry to a real boundary if its terminal allows.
fn record_finished_turn(state: &mut WorkerState, turn_id: &str, completed: bool) {
    if let Some(turn) = state
        .finished_turns
        .iter_mut()
        .find(|turn| turn.turn_id == turn_id)
    {
        turn.completed |= completed;
    } else {
        state.finished_turns.push(FinishedTurn {
            turn_id: turn_id.to_owned(),
            completed,
        });
    }
}

/// Goddard's modes onto the closed MSP vocabulary. Muse cannot split "edit
/// yes, command ask", so `AutoAcceptEdits` lands on the same rung as `Ask`;
/// `Auto` maps to the rung that lets policy decide without prompting.
fn approval_mode(mode: RuntimeMode) -> &'static str {
    match mode {
        RuntimeMode::Ask | RuntimeMode::AutoAcceptEdits => "promptUnmatched",
        RuntimeMode::Auto => "onRequest",
        RuntimeMode::FullAccess => "allowAll",
    }
}

/// The reverse of [`approval_mode`]: the host's effective mode back onto
/// Goddard's vocabulary. `denyUnmatched` has no analogue — it lands on the
/// closest restrictive rung.
fn runtime_mode(mode: &str) -> Option<RuntimeMode> {
    match mode {
        "promptUnmatched" | "denyUnmatched" => Some(RuntimeMode::Ask),
        "onRequest" => Some(RuntimeMode::Auto),
        "allowAll" => Some(RuntimeMode::FullAccess),
        _ => None,
    }
}

/// The closed MSP effort vocabulary; anything else is omitted rather than
/// rejected by the host as invalid params.
fn reasoning_effort(effort: Option<&str>) -> Option<&str> {
    match effort? {
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "ultra" => effort,
        _ => None,
    }
}

fn text_parts(text: &str) -> Value {
    json!([{ "type": "text", "text": text }])
}

/// Largest file inlined into an `image` part. The schema sets no bound, but
/// base64 inflates the payload on a line the host must read whole; anything
/// bigger keeps only its `@mention` path text like every other attachment.
const MAX_IMAGE_PART_BYTES: usize = 8 * 1024 * 1024;

/// A turn submission's ordered content parts: prompt text first, then one
/// `image` part per staged image attachment the file system can still
/// deliver. Width/height stay off — MSP accepts them only as a pair, and
/// decoding dimensions just to fill them buys nothing.
fn input_parts(text: &str, attachments: &[MessageAttachment]) -> Value {
    let mut parts = vec![json!({ "type": "text", "text": text })];
    for attachment in attachments {
        if let Some(part) = image_part(attachment) {
            parts.push(part);
        }
    }
    Value::Array(parts)
}

fn image_part(attachment: &MessageAttachment) -> Option<Value> {
    if !attachment.is_image {
        return None;
    }
    let media_type = match attachment
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => return None,
    };
    let bytes = std::fs::read(&attachment.path).ok()?;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_PART_BYTES {
        return None;
    }
    Some(json!({
        "type": "image",
        "mediaType": media_type,
        "base64Data": base64::engine::general_purpose::STANDARD.encode(bytes),
    }))
}

fn muse_error(context: &str, error: &MuseError) -> anyhow::Error {
    let message = error.message();
    if error.is_auth_failure() {
        anyhow!(
            "{context}: {message}. Run `muse login` with the configured Muse binary, then retry."
        )
    } else {
        anyhow!("{context}: {message}")
    }
}

fn handle_command(worker: &Worker, message: DriverCommand, state: &mut WorkerState) -> bool {
    match message {
        DriverCommand::Prompt { text, attachments } => {
            let mut params = json!({
                "commandId": worker.service.mint_command_id(),
                "sessionId": state.session_id,
                "input": input_parts(&text, &attachments),
                "displayText": text,
                // A second prompt during a running turn waits in the host's
                // queue rather than erroring, matching steer-less providers.
                "ifBusy": "queue",
            });
            if let Some(effort) = reasoning_effort(state.reasoning_effort.as_deref()) {
                params["reasoningEffort"] = json!(effort);
            }
            match worker.service.call("turn/start", params) {
                Ok(result) => {
                    // `disposition` is authoritative: a queued submission
                    // emits no TurnStarted until `turn/started` arrives at
                    // launch, so the local turn does not count as
                    // provider-started early.
                    let disposition = result.get("disposition").and_then(Value::as_str);
                    let started = disposition == Some("started")
                        || result.get("startedNewTurn").and_then(Value::as_bool) == Some(true);
                    if started {
                        if let Some(turn_id) = result.get("turnId").and_then(Value::as_str) {
                            state.active_turn = Some(turn_id.to_owned());
                        }
                        let _ = worker.events.send(DriverEvent::TurnStarted);
                    } else if disposition == Some("queued")
                        && let Some(turn_id) = result.get("turnId").and_then(Value::as_str)
                    {
                        state.queued_turns.push(turn_id.to_owned());
                    }
                }
                Err(error) => {
                    let _ = worker.events.send(DriverEvent::Error(
                        muse_error("Muse Code rejected the prompt", &error).to_string(),
                    ));
                    let _ = worker.events.send(DriverEvent::turn_finished_keyed(
                        false,
                        localized!("errors.provider_start_turn", provider = "Muse Code"),
                    ));
                }
            }
        }
        DriverCommand::Steer(text) => {
            let Some(turn_id) = state.active_turn.clone() else {
                let _ = worker.events.send(DriverEvent::steer_rejected_keyed(
                    text,
                    localized!("errors.provider_no_active_turn", provider = "Muse Code"),
                ));
                return true;
            };
            let mut params = json!({
                "commandId": worker.service.mint_command_id(),
                "sessionId": state.session_id,
                "expectedTurnId": turn_id,
                "input": text_parts(&text),
            });
            if let Some(effort) = reasoning_effort(state.reasoning_effort.as_deref()) {
                params["reasoningEffort"] = json!(effort);
            }
            match worker.service.call("turn/steer", params) {
                Ok(_) => {
                    let _ = worker.events.send(DriverEvent::SteerAccepted {
                        message: text,
                        sent_by_task: None,
                    });
                }
                Err(error) => {
                    let _ = worker.events.send(DriverEvent::SteerRejected {
                        message: text,
                        reason: error.message(),
                        reason_i18n: None,
                    });
                }
            }
        }
        DriverCommand::Cancel => {
            let mut params = json!({
                "commandId": worker.service.mint_command_id(),
                "sessionId": state.session_id,
            });
            if let Some(turn_id) = state.active_turn.as_deref() {
                params["turnId"] = json!(turn_id);
            }
            // `turn/interrupt` is the priority-lane stop gesture the schema
            // defines for "the user pressed stop"; no retract pairing —
            // Goddard keeps the submission in the transcript.
            if let Err(error) = worker.service.call("turn/interrupt", params) {
                let _ = worker.events.send(DriverEvent::localized_error(localized!(
                    "errors.stop_provider",
                    provider = "Muse Code",
                    error = error.message()
                )));
            }
            // Interrupt stops only the foreground turn; submits the host
            // already queued would still launch after it. Reclaim each one
            // this driver admitted — per the schema an admitted reclaim
            // means the turn will not launch, so the ack settles the shell
            // and the `turn/unqueued` event then finds nothing tracked.
            for turn_id in std::mem::take(&mut state.queued_turns) {
                match worker.service.call(
                    "turn/unqueue",
                    json!({
                        "commandId": worker.service.mint_command_id(),
                        "sessionId": state.session_id,
                        "turnId": turn_id,
                    }),
                ) {
                    Ok(_) => {
                        let _ = worker.events.send(DriverEvent::TurnFinished {
                            success: false,
                            summary: None,
                            summary_i18n: None,
                        });
                    }
                    Err(error) => {
                        // Keep the id tracked so a later stop retries; a
                        // `turn/started` or `turn/unqueued` event drops it.
                        state.queued_turns.push(turn_id);
                        let _ = worker.events.send(DriverEvent::localized_error(localized!(
                            "errors.stop_provider",
                            provider = "Muse Code",
                            error = error.message()
                        )));
                    }
                }
            }
        }
        DriverCommand::Respond {
            request_id,
            option_id,
        } => {
            let Some(pending) = state.approvals.remove(&request_id) else {
                return true;
            };
            if let Err(error) = worker.service.call(
                "approval/decide",
                json!({
                    "commandId": worker.service.mint_command_id(),
                    "sessionId": state.session_id,
                    "approvalId": request_id,
                    "choiceId": option_id,
                    "requirementId": pending.requirement_id,
                }),
            ) {
                let _ = worker.events.send(DriverEvent::localized_error(localized!(
                    "errors.answer_provider_permission",
                    provider = "Muse Code",
                    error = error.message()
                )));
            }
        }
        DriverCommand::RespondUserInput {
            request_id,
            answers,
        } => {
            let Some(pending) = state.user_inputs.remove(&request_id) else {
                return true;
            };
            let answers = user_input_answers(&pending.questions, &answers);
            if let Err(error) = worker.service.call(
                "userInput/answer",
                json!({
                    "commandId": worker.service.mint_command_id(),
                    "sessionId": state.session_id,
                    "userInputId": request_id,
                    "answers": answers,
                }),
            ) {
                let _ = worker.events.send(DriverEvent::localized_error(localized!(
                    "errors.answer_provider_question",
                    provider = "Muse Code",
                    error = error.message()
                )));
            }
        }
        DriverCommand::ClarifyUserInput {
            request_id,
            content,
        } => {
            if state.user_inputs.remove(&request_id).is_none() {
                return true;
            }
            // `clarification` settles the request like an answer, but the
            // model reads it as "let me explain" and re-decides the
            // question — content is capped at 500 chars like freeText.
            if let Err(error) = worker.service.call(
                "userInput/clarify",
                json!({
                    "commandId": worker.service.mint_command_id(),
                    "sessionId": state.session_id,
                    "userInputId": request_id,
                    "clarification": {
                        "format": "text",
                        "content": content.chars().take(500).collect::<String>(),
                    },
                }),
            ) {
                let _ = worker.events.send(DriverEvent::localized_error(localized!(
                    "errors.answer_provider_question",
                    provider = "Muse Code",
                    error = error.message()
                )));
            }
        }
        DriverCommand::CancelUserInput { request_id } => {
            if state.user_inputs.remove(&request_id).is_none() {
                return true;
            }
            if let Err(error) = worker.service.call(
                "userInput/cancel",
                json!({
                    "commandId": worker.service.mint_command_id(),
                    "sessionId": state.session_id,
                    "userInputId": request_id,
                }),
            ) {
                let _ = worker.events.send(DriverEvent::localized_error(localized!(
                    "errors.answer_provider_question",
                    provider = "Muse Code",
                    error = error.message()
                )));
            }
        }
        DriverCommand::ApplyOptions(options, reply) => {
            let mut applied = true;
            if options.model != state.model {
                let model = match options.model.as_deref() {
                    Some(model) => json!({ "modelId": model }),
                    None => Value::Null,
                };
                if model.is_null()
                    || worker
                        .service
                        .call(
                            "session/setModel",
                            json!({
                                "commandId": worker.service.mint_command_id(),
                                "sessionId": state.session_id,
                                "model": model,
                            }),
                        )
                        .is_err()
                {
                    applied = options.model == state.model;
                } else {
                    state.model = options.model.clone();
                }
            }
            if options.mode != state.mode {
                match worker.service.call(
                    "session/setApprovalMode",
                    json!({
                        "commandId": worker.service.mint_command_id(),
                        "sessionId": state.session_id,
                        "mode": approval_mode(options.mode),
                    }),
                ) {
                    Ok(_) => state.mode = options.mode,
                    Err(error) => {
                        let _ = worker.events.send(DriverEvent::Error(format!(
                            "Muse Code: {}",
                            error.message()
                        )));
                    }
                }
            }
            state.reasoning_effort = options.reasoning_effort;
            let _ = reply.send(applied);
        }
        DriverCommand::Fork { turns, reply } => {
            let _ = reply.send(fork_session(worker, state, turns));
        }
        DriverCommand::Rollback { turns, reply } => {
            // Rewind moves THIS driver onto the fork, so the reply waits for
            // the attach: an Ok the daemon acts on must mean the worker
            // already listens to the new session, and a failed attach leaves
            // the driver fully on the source session.
            let result = fork_session(worker, state, turns).and_then(|cursor| {
                let ProviderResumeCursor::Muse { session_id, .. } = &cursor else {
                    unreachable!()
                };
                switch_session(worker, state, session_id.clone()).map(|()| cursor)
            });
            let _ = reply.send(result);
        }
        DriverCommand::Shutdown => return false,
    }
    true
}

/// Move the worker onto a forked session. The local subscription exists
/// BEFORE `session/resume` (the host auto-subscribes the connection and
/// can emit the session's first events before the response lands), and
/// state only commits after the attach succeeds — a failed resume leaves
/// the driver fully on the source session.
fn switch_session(
    worker: &Worker,
    state: &mut WorkerState,
    session_id: String,
) -> anyhow::Result<()> {
    let subscription = worker.service.subscribe(&session_id);
    let result = worker
        .service
        .call(
            "session/resume",
            json!({
                "commandId": worker.service.mint_command_id(),
                "sessionId": session_id,
                "history": "snapshot",
            }),
        )
        .map_err(|error| muse_error("could not attach to the forked Muse session", &error))?;
    // `view/unsubscribe` is a request, not a notification — the reply is
    // the empty object and is deliberately ignored. Only sent now that the
    // attach is known to have succeeded.
    let _ = worker
        .service
        .call("view/unsubscribe", json!({ "sessionId": state.session_id }));
    state.subscription = subscription;
    state.session_id = session_id.clone();
    state.active_turn = None;
    state.queued_turns.clear();
    state.finished_turns = finished_turns(&worker.service, &session_id).unwrap_or_default();
    state.retried_turns.clear();
    state.items.clear();
    state.approvals.clear();
    state.user_inputs.clear();
    restore_snapshot(&result, state, &worker.events);
    let _ = worker.events.send(DriverEvent::Connected {
        provider_cursor: Some(ProviderResumeCursor::Muse {
            session_id,
            view_cursor: result
                .get("viewCursor")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
    });
    Ok(())
}

fn fork_session(
    worker: &Worker,
    state: &WorkerState,
    turns_to_remove: usize,
) -> anyhow::Result<ProviderResumeCursor> {
    // Boundary math runs in provider-started turns: finished turns plus the
    // in-flight one, whose `turn/completed` has not arrived yet.
    let mut turns = state.finished_turns.clone();
    if let Some(active) = state.active_turn.as_deref()
        && !turns.iter().any(|turn| turn.turn_id == active)
    {
        turns.push(FinishedTurn {
            turn_id: active.to_owned(),
            completed: false,
        });
    }
    let retained = turns.len().checked_sub(turns_to_remove).ok_or_else(|| {
        anyhow!(
            "Muse Code has only {} provider turns, but Goddard needs to remove {turns_to_remove}",
            turns.len()
        )
    })?;
    // A cut point names the last completed turn to copy, inclusive. Nothing
    // retained means "before the first turn", which MSP cannot express.
    let Some(last_turn_id) = fork_boundary_id(&turns, retained) else {
        bail!("Muse Code cannot fork to before its first completed turn");
    };
    let params = json!({
        "commandId": worker.service.mint_command_id(),
        "sessionId": state.session_id,
        // The driver already knows the boundary; the result's items are the
        // fork's transcript, which the daemon never reads.
        "excludeItems": true,
        "cutPoint": { "lastTurnId": last_turn_id },
    });
    let result = worker
        .service
        .call("session/fork", params)
        .map_err(|error| muse_error("could not fork the Muse Code session", &error))?;
    let session_id = result
        .pointer("/session/sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Muse Code returned no forked session ID"))?;
    Ok(ProviderResumeCursor::Muse {
        session_id: session_id.to_owned(),
        view_cursor: result
            .get("viewCursor")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// One view notification or republished server request for this session.
/// `service` is `None` only in tests, where `view/gap` fill has no host.
fn handle_event(
    service: Option<&MuseService>,
    events: &DriverEventSender,
    state: &mut WorkerState,
    method: &str,
    params: &Value,
) {
    match method {
        // `session/started` is the broadcast for a NEW session; a driver
        // already knows its own lifecycle result.
        "session/started" | "initialized" => {}
        "turn/started" => {
            let turn_id = params.get("turnId").and_then(Value::as_str);
            if let Some(turn_id) = turn_id {
                // A queued submit reaching its launch boundary leaves the
                // reclaimable set.
                state.queued_turns.retain(|queued| queued != turn_id);
            }
            state.active_turn = turn_id.map(str::to_owned);
            let _ = events.send(DriverEvent::TurnStarted);
        }
        "turn/completed" => {
            if let Some(turn_id) = params.get("turnId").and_then(Value::as_str) {
                state.queued_turns.retain(|queued| queued != turn_id);
                if state.active_turn.as_deref() == Some(turn_id) {
                    state.active_turn = None;
                }
                record_finished_turn(
                    state,
                    turn_id,
                    params.get("terminal").and_then(Value::as_str) == Some("completed"),
                );
                if state.retried_turns.remove(turn_id) {
                    let _ = events.send(DriverEvent::RichActivity(
                        ActivityItem::new(
                            Some(format!("muse-retry:{turn_id}")),
                            ActivityKind::Tool,
                            "Model call recovered",
                            None,
                            true,
                        )
                        .with_tool_name(Some("muse-retry")),
                    ));
                }
            }
            let (success, summary) = turn_outcome(params);
            let _ = events.send(DriverEvent::TurnFinished {
                success,
                summary,
                summary_i18n: None,
            });
        }
        // A retracted turn ran, then its output was withdrawn — it still
        // counts toward the provider-turn index but can never be a fork
        // boundary. `turn/unqueued` names a queued submission reclaimed
        // before launch. Neither emits `turn/completed`, so both settle the
        // open shell here — otherwise the turn spins forever.
        "turn/retracted" | "turn/unqueued" => {
            if let Some(turn_id) = params.get("turnId").and_then(Value::as_str) {
                if method == "turn/unqueued"
                    && state.active_turn.as_deref() != Some(turn_id)
                    && !state.queued_turns.iter().any(|queued| queued == turn_id)
                {
                    // A reclaim naming nothing this driver admitted belongs
                    // to another client's queue; no local turn is open, so
                    // settling here would fail the wrong turn.
                    return;
                }
                state.queued_turns.retain(|queued| queued != turn_id);
                if state.active_turn.as_deref() == Some(turn_id) {
                    state.active_turn = None;
                }
                if method == "turn/retracted" {
                    record_finished_turn(state, turn_id, false);
                }
            }
            let _ = events.send(DriverEvent::TurnFinished {
                success: false,
                summary: None,
                summary_i18n: None,
            });
        }
        "session/branchChanged" => {}
        "turn/retryScheduled" => {
            let turn_id = params.get("turnId").and_then(Value::as_str).unwrap_or("");
            let next = params
                .get("nextAttempt")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let max = params
                .get("maxAttempts")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let reason = params.get("reason").and_then(Value::as_str);
            let title = if max > 0 {
                format!("Model call failed — retrying ({next}/{max})")
            } else {
                "Model call failed — retrying".to_owned()
            };
            state.retried_turns.insert(turn_id.to_owned());
            let _ = events.send(DriverEvent::RichActivity(
                ActivityItem::new(
                    Some(format!("muse-retry:{turn_id}")),
                    ActivityKind::Tool,
                    title,
                    reason.map(|r| truncate(r, MAX_DETAIL_CHARS)),
                    false,
                )
                .with_tool_name(Some("muse-retry")),
            ));
        }
        "item/started" | "item/updated" | "item/completed" => {
            if let Some(item) = params.get("item") {
                handle_item(events, state, item, method == "item/completed");
            }
        }
        "item/delta" => handle_item_delta(events, state, params),
        "approval/requested" | "approval/request" | "approval/updated" => {
            let approval_id = params
                .get("approvalId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if approval_id.is_empty() {
                return;
            }
            let requirement_id = params
                .get("currentRequirementId")
                .cloned()
                .unwrap_or(Value::Null);
            state
                .approvals
                .insert(approval_id.clone(), PendingApproval { requirement_id });
            // `Auto` never answers blindly: the review replies when a backend
            // is configured and the user does when it is not.
            if state.mode == RuntimeMode::Auto
                && let (Some(service), Some(eval)) = (service, state.eval.clone())
            {
                let allow_choice = approval_allow_choice(params);
                let action = crate::permission_review::PendingAction {
                    provider: "muse",
                    tool: params
                        .get("toolName")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_owned(),
                    arguments: params.to_string(),
                    call_id: approval_id.clone(),
                    detail: params
                        .get("rawArgs")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                };
                let events = events.clone();
                let service = service.clone();
                let session_id = state.session_id.clone();
                let params = params.clone();
                crate::permission_review::review_on_thread(eval, action, move |verdict| {
                    let decided = verdict == crate::permission_review::ReviewVerdict::Allow
                        && allow_choice.is_some();
                    if decided {
                        let _ = service.call(
                            "approval/decide",
                            json!({
                                "commandId": service.mint_command_id(),
                                "sessionId": session_id,
                                "approvalId": approval_id,
                                "choiceId": allow_choice.unwrap_or_default(),
                                "requirementId": params
                                    .get("currentRequirementId")
                                    .cloned()
                                    .unwrap_or(Value::Null),
                            }),
                        );
                    } else {
                        emit_permission(&events, &params, &approval_id);
                    }
                });
            } else {
                emit_permission(events, params, &approval_id);
            }
        }
        "approval/resolved" => {
            if let Some(id) = params.get("approvalId").and_then(Value::as_str) {
                state.approvals.remove(id);
            }
        }
        "userInput/requested" | "userInput/request" => {
            let request_id = params
                .get("userInputId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if request_id.is_empty() {
                return;
            }
            let raw_questions = params
                .get("questions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            state.user_inputs.insert(
                request_id.clone(),
                PendingUserInput {
                    questions: raw_questions.clone(),
                },
            );
            let questions = raw_questions
                .iter()
                .filter_map(user_input_question)
                .collect::<Vec<_>>();
            if questions.is_empty() {
                let _ = events.send(DriverEvent::Error(
                    "Muse Code issued a question request with no usable questions".to_owned(),
                ));
                return;
            }
            let _ = events.send(DriverEvent::UserInputRequested {
                request_id,
                questions,
            });
        }
        "userInput/settled" => {
            if let Some(id) = params.get("userInputId").and_then(Value::as_str) {
                state.user_inputs.remove(id);
            }
        }
        "session/contextUsage" => {
            let _ = events.send(DriverEvent::UsageUpdated {
                context_tokens: params.get("usedTokens").and_then(Value::as_u64),
                context_window: params.get("windowTokens").and_then(Value::as_u64),
            });
        }
        // Per-completion counters; `session/contextUsage` already carries
        // window pressure, which is what the UI displays.
        "session/tokenUsage" => {}
        "session/goalChanged" => {
            let goal = params.get("goal").filter(|goal| !goal.is_null());
            let _ = events.send(DriverEvent::GoalUpdated(goal.and_then(muse_goal)));
        }
        "session/todoListChanged" => {
            emit_todo_list(events, params);
        }
        "session/modelChanged" => {
            state.model = params
                .get("modelId")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        "session/approvalModeChanged" => {
            // The mode can change outside ApplyOptions (hotkey, config
            // reload) — resync so a later ApplyOptions does not skip the
            // `session/setApprovalMode` the drift actually needs.
            if let Some(mode) = params
                .get("mode")
                .and_then(Value::as_str)
                .and_then(runtime_mode)
            {
                state.mode = mode;
            }
        }
        // A dropped delivery hole: durable events between `after` and `next`
        // are re-read through `view/page` and fed back through this handler.
        // `item/delta` never pages, so text lost in a gap is repaired by the
        // item's authoritative `item/completed` text.
        "view/gap" => {
            if let Some(service) = service {
                gap_fill(service, events, state, params);
            }
        }
        _ => {}
    }
}

fn handle_item(events: &DriverEventSender, state: &mut WorkerState, item: &Value, completed: bool) {
    let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
    let item_id = item.get("itemId").and_then(Value::as_str).unwrap_or("");
    let status = item.get("status").and_then(Value::as_str).unwrap_or("");
    // The status vocabulary is open — anything other than `inProgress` is
    // terminal, so `rejected`/`timedOut`/future statuses settle the card.
    let terminal = completed || (!status.is_empty() && status != "inProgress");

    match kind {
        "userMessage" => {}
        "agentMessage" => {
            let entry = state.items.entry(item_id.to_owned()).or_insert(ItemState {
                kind: kind.to_owned(),
                streamed: 0,
            });
            entry.kind = kind.to_owned();
            if let Some(text) = item.get("text").and_then(Value::as_str)
                && text.len() > entry.streamed
                && let Some(suffix) = text.get(entry.streamed..)
            {
                entry.streamed = text.len();
                if !suffix.is_empty() {
                    let _ = events.send(DriverEvent::TextDelta(suffix.to_owned()));
                }
            }
        }
        "reasoning" => {
            let entry = state.items.entry(item_id.to_owned()).or_insert(ItemState {
                kind: kind.to_owned(),
                streamed: 0,
            });
            entry.kind = kind.to_owned();
            // Parts join WITHOUT a separator: `summary.n` deltas concatenate
            // to each part's exact bytes, so `streamed` (a raw delta byte
            // count) only lines up with the join when nothing is inserted.
            let text = item
                .get("summary")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("")
                })
                .or_else(|| item.get("text").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_default();
            if text.len() > entry.streamed
                && let Some(suffix) = text.get(entry.streamed..)
            {
                entry.streamed = text.len();
                let _ = events.send(DriverEvent::ReasoningDelta(suffix.to_owned()));
            }
        }
        _ => {
            if item_id.is_empty() {
                return;
            }
            state.items.entry(item_id.to_owned()).or_insert(ItemState {
                kind: kind.to_owned(),
                streamed: 0,
            });
            if let Some(activity) = item_activity(item, kind, terminal) {
                let _ = events.send(DriverEvent::RichActivity(activity));
            }
        }
    }
}

/// A non-message item as an activity card. Unknown kinds get the host's
/// `fallbackText` — MSP's item vocabulary is open and a missing kind must
/// never swallow the event.
fn item_activity(item: &Value, kind: &str, terminal: bool) -> Option<ActivityItem> {
    let item_id = item.get("itemId").and_then(Value::as_str)?;
    let failed = matches!(item.get("status").and_then(Value::as_str), Some("failed"))
        || item.get("failureReason").is_some_and(|r| !r.is_null());
    let detail = item
        .get("failureReason")
        .and_then(Value::as_str)
        .or_else(|| item.get("visibleOutput").and_then(Value::as_str))
        .or_else(|| {
            // `result` is a string for some kinds and a SubagentResult
            // object for subagents — read its summary/text fields too.
            item.get("result").and_then(|result| {
                result.as_str().or_else(|| {
                    result
                        .get("summary")
                        .and_then(Value::as_str)
                        .or_else(|| result.get("text").and_then(Value::as_str))
                })
            })
        })
        .or_else(|| item.get("message").and_then(Value::as_str))
        .map(|text| truncate(text, MAX_DETAIL_CHARS));
    let args = item.get("args").and_then(Value::as_str);

    let (activity_kind, title) = match kind {
        "toolCall" | "userShell" => {
            let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
            (
                if kind == "userShell" {
                    ActivityKind::Command
                } else {
                    ActivityKind::from_tool_name(tool)
                },
                tool_title(tool, args)
                    .or_else(|| {
                        item.get("commandText")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| tool.to_owned()),
            )
        }
        "subagent" => (
            ActivityKind::Tool,
            item.get("objective")
                .and_then(Value::as_str)
                .map(|objective| format!("Subagent: {objective}"))
                .or_else(|| {
                    item.get("agentPath")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "Subagent".to_owned()),
        ),
        "workflow" => (
            ActivityKind::Tool,
            item.get("objective")
                .and_then(Value::as_str)
                .map(|objective| format!("Workflow: {objective}"))
                .unwrap_or_else(|| "Workflow".to_owned()),
        ),
        "compaction" => (ActivityKind::Plan, "Context compacted".to_owned()),
        "reminderChild" => (
            ActivityKind::Tool,
            item.get("objective")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| "Reminder".to_owned()),
        ),
        _ => (
            ActivityKind::Tool,
            item.get("fallbackText")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Muse {kind}")),
        ),
    };

    let args_value = args.and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    let output = detail.map(|text| Value::String(text));
    let mut activity = activity::tool_activity(
        Some(item_id.to_owned()),
        activity_kind,
        title,
        args_value.as_ref(),
        output.as_ref(),
        None,
        failed,
        terminal,
    );
    if let Some(tool) = item.get("tool").and_then(Value::as_str) {
        activity = activity.with_tool_name(Some(tool));
    }
    Some(activity)
}

/// A tool card title: prefer a readable subject pulled out of the raw args
/// (command, path, query) before falling back to the tool name.
fn tool_title(tool: &str, args: Option<&str>) -> Option<String> {
    let args = args.and_then(|raw| serde_json::from_str::<Value>(raw).ok())?;
    for key in [
        "command",
        "cmd",
        "file_path",
        "filePath",
        "path",
        "pattern",
        "query",
        "url",
        "description",
    ] {
        if let Some(value) = args.get(key).and_then(Value::as_str)
            && !value.trim().is_empty()
        {
            return Some(format!("{tool}: {}", truncate(value.trim(), 200)));
        }
    }
    Some(tool.to_owned())
}

fn handle_item_delta(events: &DriverEventSender, state: &mut WorkerState, params: &Value) {
    let Some(item_id) = params.get("itemId").and_then(Value::as_str) else {
        return;
    };
    let Some(delta) = params.get("delta").and_then(Value::as_str) else {
        return;
    };
    if delta.is_empty() {
        return;
    }
    let field = params
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or("text");
    let kind = state
        .items
        .get(item_id)
        .map(|item| item.kind.as_str())
        .unwrap_or("agentMessage");
    match kind {
        // `summary.n` addresses reasoning-part boundaries; the field's index
        // does not matter for appending.
        "reasoning" if field == "text" || field.starts_with("summary") => {
            if let Some(entry) = state.items.get_mut(item_id) {
                entry.streamed += delta.len();
            }
            let _ = events.send(DriverEvent::ReasoningDelta(delta.to_owned()));
        }
        // Tool output streams onto the card's completed detail; streaming it
        // live would need partial-activity updates the activity feed replaces
        // anyway on `item/updated`/`item/completed`.
        "toolCall" | "userShell" => {}
        _ if field == "text" => {
            let entry = state.items.entry(item_id.to_owned()).or_insert(ItemState {
                kind: "agentMessage".to_owned(),
                streamed: 0,
            });
            entry.streamed += delta.len();
            let _ = events.send(DriverEvent::TextDelta(delta.to_owned()));
        }
        _ => {}
    }
}

/// The review's grant is at most allow-once — it picks the host's own
/// approved choice and never asks for a durable grant.
fn approval_allow_choice(params: &Value) -> Option<String> {
    params
        .get("availableChoices")
        .and_then(Value::as_array)?
        .iter()
        .find(|choice| {
            choice
                .get("decision")
                .and_then(Value::as_str)
                .is_some_and(|decision| decision.starts_with("approved"))
        })?
        .get("choiceId")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn emit_permission(events: &DriverEventSender, params: &Value, approval_id: &str) {
    let subject = params.get("subject").cloned().unwrap_or(Value::Null);
    let tool_name = params.get("toolName").and_then(Value::as_str).unwrap_or("");
    let title = permission_title(&subject, tool_name);
    let detail = params
        .get("rawArgs")
        .and_then(Value::as_str)
        .map(|args| truncate(args, 2000))
        .unwrap_or_default();
    let options = params
        .get("availableChoices")
        .and_then(Value::as_array)
        .map(|choices| {
            choices
                .iter()
                .filter_map(|choice| {
                    let choice_id = choice.get("choiceId").and_then(Value::as_str)?;
                    let label = choice
                        .get("label")
                        .and_then(Value::as_str)
                        .unwrap_or(choice_id);
                    let decision = choice.get("decision").and_then(Value::as_str).unwrap_or("");
                    Some(PermissionOption {
                        id: choice_id.to_owned(),
                        label: label.to_owned(),
                        label_i18n: None,
                        allow: decision.starts_with("approved"),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if options.is_empty() {
        // The approval stays pending host-side; a silent return would hang
        // the session on a prompt the user can never see.
        let _ = events.send(DriverEvent::Error(
            "Muse Code issued an approval request with no usable choices".to_owned(),
        ));
        return;
    }
    let _ = events.send(DriverEvent::Permission {
        request_id: approval_id.to_owned(),
        title,
        detail,
        options,
        title_i18n: None,
        detail_i18n: None,
    });
}

fn permission_title(subject: &Value, tool_name: &str) -> String {
    let kind = subject
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let target = [
        subject.get("command"),
        subject.get("path"),
        subject.get("host"),
        subject.get("target"),
        subject.get("toolName"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str)
    .unwrap_or(tool_name);
    match kind {
        "shell" => format!("Run shell command: {}", truncate(target, 120)),
        "fileAccess" => format!("Access file: {}", truncate(target, 120)),
        "network" => format!("Network access: {}", truncate(target, 120)),
        "process" => format!("Launch process: {}", truncate(target, 120)),
        _ if !target.is_empty() => format!("{tool_name}: {}", truncate(target, 120)),
        _ => format!("Allow {tool_name}"),
    }
}

fn user_input_question(question: &Value) -> Option<UserInputQuestion> {
    let id = question.get("id").and_then(Value::as_str)?;
    let options = question
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(|option| {
                    Some(UserInputOption {
                        label: option.get("label").and_then(Value::as_str)?.to_owned(),
                        description: option
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(UserInputQuestion {
        id: id.to_owned(),
        header: question
            .get("header")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        question: question
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        options,
        multi_select: question.pointer("/selection/mode").and_then(Value::as_str)
            == Some("multiple"),
    })
}

/// `UserInputAnswer.answers` is flat strings; the wire shape depends on the
/// question's selection mode, so the stored questions pick the encoding.
fn user_input_answers(questions: &[Value], answers: &[UserInputAnswer]) -> Value {
    let mut out = Vec::new();
    for answer in answers {
        let question = questions.iter().find(|question| {
            question.get("id").and_then(Value::as_str) == Some(answer.question_id.as_str())
        });
        let has_options = question
            .and_then(|q| q.get("options"))
            .and_then(Value::as_array)
            .is_some_and(|options| !options.is_empty());
        let multi = question
            .and_then(|q| q.pointer("/selection/mode"))
            .and_then(Value::as_str)
            == Some("multiple");
        let mut entry = json!({ "questionId": answer.question_id });
        if has_options && multi {
            entry["selectedLabels"] = json!(answer.answers);
        } else if has_options {
            if let Some(first) = answer.answers.first() {
                entry["selectedLabel"] = json!(first);
            }
        } else if !answer.answers.is_empty() {
            // The wire caps freeText at 500 chars.
            entry["freeText"] = json!(
                answer
                    .answers
                    .join("\n")
                    .chars()
                    .take(500)
                    .collect::<String>()
            );
        }
        out.push(entry);
    }
    Value::Array(out)
}

fn muse_goal(goal: &Value) -> Option<ThreadGoal> {
    let objective = goal.get("objective").and_then(Value::as_str)?;
    let status = match goal.get("status").and_then(Value::as_str).unwrap_or("") {
        "completed" | "complete" | "done" | "cancelled" | "stopped" | "abandoned" => {
            ThreadGoalStatus::Complete
        }
        "paused" => ThreadGoalStatus::Paused,
        _ => ThreadGoalStatus::Active,
    };
    Some(ThreadGoal {
        objective: objective.to_owned(),
        status,
        token_budget: None,
        tokens_used: 0,
        time_used_seconds: 0,
    })
}

fn emit_todo_list(events: &DriverEventSender, params: &Value) {
    let Some(items) = params.get("items").and_then(Value::as_array) else {
        return;
    };
    let lines = items
        .iter()
        .filter_map(|item| {
            let text = item.get("text").and_then(Value::as_str)?;
            let mark = match item.get("status").and_then(Value::as_str) {
                Some("completed") => "[x]",
                Some("inProgress") => "[~]",
                Some("cancelled") => "[-]",
                _ => "[ ]",
            };
            Some(format!("{mark} {text}"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let all_done = !items.is_empty()
        && items.iter().all(|item| {
            matches!(
                item.get("status").and_then(Value::as_str),
                Some("completed") | Some("cancelled")
            )
        });
    let _ = events.send(DriverEvent::RichActivity(
        ActivityItem::new(
            Some("muse-todos".to_owned()),
            ActivityKind::Plan,
            "Task list",
            Some(lines),
            all_done,
        )
        .with_tool_name(Some("muse-todos")),
    ));
}

fn turn_outcome(params: &Value) -> (bool, Option<String>) {
    match params.get("terminal").and_then(Value::as_str) {
        Some("completed") => (true, None),
        Some("cancelled") | Some("interrupted") => (false, None),
        _ => {
            let error = params.get("error");
            let message = error
                .and_then(|error| error.get("message").and_then(Value::as_str))
                .or_else(|| params.get("reason").and_then(Value::as_str))
                .unwrap_or("Muse Code turn failed");
            (false, Some(message.to_owned()))
        }
    }
}

/// Splice-fill a `view/gap`: page forward from `after` and feed each durable
/// event back through the handler until the `next` cursor is reached.
fn gap_fill(
    service: &MuseService,
    events: &DriverEventSender,
    state: &mut WorkerState,
    params: &Value,
) {
    let mut after = params
        .get("after")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let next = params
        .get("next")
        .and_then(Value::as_str)
        .map(str::to_owned);
    for _ in 0..32 {
        let mut page_params = json!({
            "sessionId": state.session_id,
            "direction": "forward",
            "limit": 500,
        });
        // `ViewPageParams.cursor` is a plain string; absent pages from the
        // start, an explicit null is invalid params.
        if let Some(cursor) = after.as_deref() {
            page_params["cursor"] = json!(cursor);
        }
        let result = service.call("view/page", page_params);
        let Ok(result) = result else { return };
        let page = result
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut reached = false;
        for event in &page {
            let event_cursor = event.pointer("/params/viewCursor").and_then(Value::as_str);
            if next.as_deref().is_some() && event_cursor == next.as_deref() {
                reached = true;
                break;
            }
            if let (Some(method), Some(params)) = (
                event.get("method").and_then(Value::as_str),
                event.get("params"),
            ) {
                handle_event(Some(service), events, state, method, params);
            }
            after = event_cursor.map(str::to_owned).or(after);
        }
        if reached || page.is_empty() {
            return;
        }
        match result.get("nextCursor").and_then(Value::as_str) {
            Some(cursor) => after = Some(cursor.to_owned()),
            None => return,
        }
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

impl DriverControl for MuseDriver {
    fn prompt(&self, prompt: String) {
        self.prompt_with_attachments(prompt, Vec::new());
    }

    fn prompt_with_attachments(&self, prompt: String, attachments: Vec<MessageAttachment>) {
        let _ = self.commands.send(DriverCommand::Prompt {
            text: prompt,
            attachments,
        });
    }

    fn supports_steer(&self) -> bool {
        true
    }

    fn steer(&self, prompt: String) {
        let _ = self.commands.send(DriverCommand::Steer(prompt));
    }

    fn cancel(&self) {
        let _ = self.commands.send(DriverCommand::Cancel);
    }

    fn respond(&self, request_id: String, option_id: String) {
        let _ = self.commands.send(DriverCommand::Respond {
            request_id,
            option_id,
        });
    }

    fn respond_user_input(&self, request_id: String, answers: Vec<UserInputAnswer>) {
        let _ = self.commands.send(DriverCommand::RespondUserInput {
            request_id,
            answers,
        });
    }

    fn supports_user_input_actions(&self) -> bool {
        true
    }

    fn clarify_user_input(&self, request_id: String, content: String) {
        let _ = self.commands.send(DriverCommand::ClarifyUserInput {
            request_id,
            content,
        });
    }

    fn cancel_user_input(&self, request_id: String) {
        let _ = self
            .commands
            .send(DriverCommand::CancelUserInput { request_id });
    }

    fn apply_options(&self, options: SessionOptions) -> bool {
        let (reply, answer) = bounded(1);
        if self
            .commands
            .send(DriverCommand::ApplyOptions(options, reply))
            .is_err()
        {
            return false;
        }
        answer.recv_timeout(ACTION_TIMEOUT).unwrap_or(false)
    }

    fn rollback(&self, turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        if turns == 0 {
            return Ok(None);
        }
        let (reply, answer) = bounded(1);
        self.commands
            .send(DriverCommand::Rollback { turns, reply })
            .map_err(|_| anyhow!("the Muse Code driver is shutting down"))?;
        answer
            .recv_timeout(ACTION_TIMEOUT)
            .map_err(|_| anyhow!("Muse Code did not answer the rewind request"))?
            .map(Some)
    }

    fn fork(&self, turns_to_remove: usize) -> anyhow::Result<ProviderResumeCursor> {
        let (reply, answer) = bounded(1);
        self.commands
            .send(DriverCommand::Fork {
                turns: turns_to_remove,
                reply,
            })
            .map_err(|_| anyhow!("the Muse Code driver is shutting down"))?;
        answer
            .recv_timeout(ACTION_TIMEOUT)
            .map_err(|_| anyhow!("Muse Code did not answer the fork request"))?
    }
}

impl Drop for MuseDriver {
    fn drop(&mut self) {
        // Shutdown wakes the worker, whose exit drops the subscription; the
        // host itself belongs to the pool and outlives any one session.
        let _ = self.commands.send(DriverCommand::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use crossbeam_channel::RecvTimeoutError;
    use uuid::Uuid;

    use super::*;
    use crate::driver::test_event_channel;
    #[cfg(unix)]
    use crate::muse_service::test_support::fake_muse;

    fn options(cwd: &Path) -> DriverStartOptions {
        DriverStartOptions {
            eval: None,
            binary: PathBuf::new(),
            cwd: cwd.to_path_buf(),
            mode: RuntimeMode::Ask,
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_window: None,
            agent_preset: None,
            computer_use_enabled: false,
            agent: None,
            subagents: None,
            provider_cursor: None,
        }
    }

    fn collect_until(
        rx: &crossbeam_channel::Receiver<DriverEvent>,
        deadline: Instant,
        done: impl Fn(&DriverEvent) -> bool,
    ) -> Vec<DriverEvent> {
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(event) => {
                    let finished = done(&event);
                    seen.push(event);
                    if finished {
                        return seen;
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        seen
    }

    #[cfg(unix)]
    #[test]
    fn muse_turn_streams_text_usage_and_completion() {
        let directory = std::env::temp_dir().join(format!("waku-muse-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut start = options(&directory);
        start.binary = fake_muse(&directory);

        let (events, rx) = test_event_channel();
        let driver = MuseDriver::start(start, events).unwrap();
        driver.prompt("hi".to_owned());
        let seen = collect_until(&rx, Instant::now() + Duration::from_secs(10), |event| {
            matches!(event, DriverEvent::TurnFinished { .. })
        });

        assert!(seen.iter().any(|event| matches!(
            event,
            DriverEvent::Connected {
                provider_cursor: Some(ProviderResumeCursor::Muse { .. })
            }
        )));
        let text: String = seen
            .iter()
            .filter_map(|event| match event {
                DriverEvent::TextDelta(delta) => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "hello world");
        assert!(seen.iter().any(|event| matches!(
            event,
            DriverEvent::UsageUpdated {
                context_tokens: Some(42),
                context_window: Some(1000),
            }
        )));
        assert!(
            seen.iter()
                .any(|event| matches!(event, DriverEvent::TurnFinished { success: true, .. }))
        );
        drop(driver);
        assert!(!directory.join("violations.log").exists());
    }

    /// A resume with no stored view cursor asks for the folded snapshot;
    /// `HistoryPreference` is a bare string on the wire, so the fake host
    /// logs a violation if the driver ever sends it as an object.
    #[cfg(unix)]
    #[test]
    fn muse_resume_requests_snapshot_history_as_a_string() {
        let directory = std::env::temp_dir().join(format!("waku-muse-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut start = options(&directory);
        start.binary = fake_muse(&directory);
        start.provider_cursor = Some(ProviderResumeCursor::Muse {
            session_id: "s-resume".to_owned(),
            view_cursor: None,
        });

        let (events, rx) = test_event_channel();
        let driver = MuseDriver::start(start, events).unwrap();
        let seen = collect_until(&rx, Instant::now() + Duration::from_secs(10), |event| {
            matches!(event, DriverEvent::Connected { .. })
        });

        assert!(seen.iter().any(|event| matches!(
            event,
            DriverEvent::Connected {
                provider_cursor: Some(ProviderResumeCursor::Muse {
                    session_id,
                    view_cursor,
                })
            } if session_id == "s-resume" && view_cursor.as_deref() == Some("c9")
        )));
        drop(driver);
        assert!(!directory.join("violations.log").exists());
    }

    /// Rewind forks the session and moves THIS driver onto the fork: the
    /// reply carries the fork cursor, a second Connected arrives, and a
    /// prompt after the rewind still runs against the new session.
    #[cfg(unix)]
    #[test]
    fn muse_rollback_attaches_the_driver_to_the_fork() {
        let directory = std::env::temp_dir().join(format!("waku-muse-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut start = options(&directory);
        start.binary = fake_muse(&directory);

        let (events, rx) = test_event_channel();
        let driver = MuseDriver::start(start, events).unwrap();
        driver.prompt("one".to_owned());
        driver.prompt("two".to_owned());
        collect_until(&rx, Instant::now() + Duration::from_secs(10), |event| {
            matches!(event, DriverEvent::TurnFinished { success: true, .. })
        });
        collect_until(&rx, Instant::now() + Duration::from_secs(10), |event| {
            matches!(event, DriverEvent::TurnFinished { success: true, .. })
        });

        let cursor = driver
            .rollback(1)
            .unwrap()
            .expect("rollback returns the fork cursor");
        let ProviderResumeCursor::Muse { session_id, .. } = &cursor else {
            panic!("expected a Muse cursor");
        };
        assert_eq!(session_id, "fork-1");

        // The attach already happened when the reply landed — a new prompt
        // must still stream against the forked session.
        driver.prompt("after rewind".to_owned());
        let seen = collect_until(&rx, Instant::now() + Duration::from_secs(10), |event| {
            matches!(event, DriverEvent::TurnFinished { .. })
        });
        assert!(seen.iter().any(|event| matches!(
            event,
            DriverEvent::Connected {
                provider_cursor: Some(ProviderResumeCursor::Muse { session_id, .. })
            } if session_id == "fork-1"
        )));
        drop(driver);
        assert!(!directory.join("violations.log").exists());
    }

    /// A submit admitted with `disposition: "queued"` sits in the host's
    /// queue, invisible to `turn/interrupt`. Stop must reclaim it with
    /// `turn/unqueue` or it launches after the user thinks they stopped.
    #[cfg(unix)]
    #[test]
    fn muse_cancel_reclaims_queued_turns() {
        let directory = std::env::temp_dir().join(format!("waku-muse-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut start = options(&directory);
        start.binary = fake_muse(&directory);
        // The marker flips the fake host's turn/start acks to `queued`.
        fs::write(directory.join("queue-all"), "").unwrap();

        let (events, rx) = test_event_channel();
        let driver = MuseDriver::start(start, events).unwrap();
        driver.prompt("queued one".to_owned());
        // Commands run in order on the worker, so the queued admission is
        // already tracked by the time Cancel is handled.
        driver.cancel();

        let seen = collect_until(&rx, Instant::now() + Duration::from_secs(10), |event| {
            matches!(event, DriverEvent::TurnFinished { .. })
        });
        assert!(
            seen.iter()
                .any(|event| matches!(event, DriverEvent::TurnFinished { success: false, .. })),
            "turn/unqueued should settle the open turn"
        );
        // The host's own queue record proves the reclaim reached it.
        let unqueued = fs::read_to_string(directory.join("unqueued.log")).unwrap_or_default();
        assert_eq!(unqueued.trim(), "t1");
        drop(driver);
        assert!(!directory.join("violations.log").exists());
    }

    fn attachment(path: &Path, is_image: bool) -> MessageAttachment {
        MessageAttachment {
            path: path.to_path_buf(),
            mention: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            is_dir: false,
            is_image,
            pasted_text_preview: None,
            blob_reference: None,
            pasted_text_preview: None,
            session_id: None,
            pasted_text_preview: None,
        }
    }

    /// Staged image attachments travel as MSP `image` parts; anything the
    /// file system or the media-type map declines keeps its `@mention`
    /// text only — the same contract text-only providers get.
    #[test]
    fn muse_input_parts_carry_image_attachments() {
        let directory = std::env::temp_dir().join(format!("waku-muse-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let png = directory.join("shot.png");
        fs::write(&png, [0x89, 0x50, 0x4e, 0x47]).unwrap();
        let text_file = directory.join("notes.txt");
        fs::write(&text_file, "hi").unwrap();

        let parts = input_parts(
            "look at this",
            &[
                attachment(&png, true),
                attachment(&text_file, false),
                attachment(&directory.join("missing.png"), true),
            ],
        );
        let parts = parts.as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], json!({"type": "text", "text": "look at this"}));
        assert_eq!(parts[1]["type"], "image");
        assert_eq!(parts[1]["mediaType"], "image/png");
        // 89 50 4e 47 is the PNG magic — the file's real bytes, encoded.
        assert_eq!(parts[1]["base64Data"], "iVBORw==");
        assert!(parts[1].get("width").is_none());
    }

    /// The fake host issues one `userInput/request` after `session/start`
    /// when its marker file exists; the driver settles it with a text
    /// clarification — the wire shape is validated by the host itself.
    #[cfg(unix)]
    #[test]
    fn muse_clarify_settles_the_question() {
        let directory = std::env::temp_dir().join(format!("waku-muse-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut start = options(&directory);
        start.binary = fake_muse(&directory);
        fs::write(directory.join("ask-question"), "").unwrap();

        let (events, rx) = test_event_channel();
        let driver = MuseDriver::start(start, events).unwrap();
        let seen = collect_until(&rx, Instant::now() + Duration::from_secs(10), |event| {
            matches!(event, DriverEvent::UserInputRequested { .. })
        });
        let request_id = seen
            .iter()
            .find_map(|event| match event {
                DriverEvent::UserInputRequested { request_id, .. } => Some(request_id.clone()),
                _ => None,
            })
            .expect("the fake host's question reached the driver");
        driver.clarify_user_input(request_id, "the second, but faster".to_owned());
        let log = wait_for_log(&directory, "userinput.log", "u1");
        assert!(log.contains("userInput/clarify"), "{log}");
        assert!(log.contains("u1"), "{log}");
        drop(driver);
        assert!(!directory.join("violations.log").exists());
    }

    /// `userInput/cancel` dismisses the request unanswered.
    #[cfg(unix)]
    #[test]
    fn muse_dismiss_cancels_the_question() {
        let directory = std::env::temp_dir().join(format!("waku-muse-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut start = options(&directory);
        start.binary = fake_muse(&directory);
        fs::write(directory.join("ask-question"), "").unwrap();

        let (events, rx) = test_event_channel();
        let driver = MuseDriver::start(start, events).unwrap();
        let seen = collect_until(&rx, Instant::now() + Duration::from_secs(10), |event| {
            matches!(event, DriverEvent::UserInputRequested { .. })
        });
        let request_id = seen
            .iter()
            .find_map(|event| match event {
                DriverEvent::UserInputRequested { request_id, .. } => Some(request_id.clone()),
                _ => None,
            })
            .expect("the fake host's question reached the driver");
        driver.cancel_user_input(request_id);
        let log = wait_for_log(&directory, "userinput.log", "u1");
        assert!(log.contains("userInput/cancel"), "{log}");
        assert!(log.contains("u1"), "{log}");
        drop(driver);
        assert!(!directory.join("violations.log").exists());
    }

    /// The host's side-effect logs land asynchronously and a line at a
    /// time; poll until the expected text is actually in the file.
    fn wait_for_log(directory: &Path, name: &str, expected: &str) -> String {
        let path = directory.join(name);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut log = String::new();
        while Instant::now() < deadline {
            if let Ok(contents) = fs::read_to_string(&path) {
                log = contents;
                if log.contains(expected) {
                    return log;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        log
    }

    /// The structured prompt path must reach the host: the fake binary
    /// validates the image part's wire shape itself.
    #[cfg(unix)]
    #[test]
    fn muse_prompt_sends_image_parts_to_the_host() {
        let directory = std::env::temp_dir().join(format!("waku-muse-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut start = options(&directory);
        start.binary = fake_muse(&directory);
        let png = directory.join("shot.png");
        fs::write(&png, [0x89, 0x50, 0x4e, 0x47]).unwrap();

        let (events, rx) = test_event_channel();
        let driver = MuseDriver::start(start, events).unwrap();
        driver.prompt_with_attachments("look".to_owned(), vec![attachment(&png, true)]);
        collect_until(&rx, Instant::now() + Duration::from_secs(10), |event| {
            matches!(event, DriverEvent::TurnFinished { .. })
        });
        drop(driver);
        assert!(!directory.join("violations.log").exists());
    }

    #[test]
    fn muse_answers_map_to_the_question_shape() {
        let questions = vec![
            json!({"id": "q1", "options": [{"label": "Yes"}, {"label": "No"}], "selection": {"mode": "single"}}),
            json!({"id": "q2", "options": [{"label": "a"}, {"label": "b"}], "selection": {"mode": "multiple"}}),
            json!({"id": "q3", "options": [], "selection": {"mode": "single"}}),
        ];
        let answers = vec![
            UserInputAnswer {
                question_id: "q1".into(),
                answers: vec!["Yes".into()],
            },
            UserInputAnswer {
                question_id: "q2".into(),
                answers: vec!["a".into(), "b".into()],
            },
            UserInputAnswer {
                question_id: "q3".into(),
                answers: vec!["free".into()],
            },
        ];
        let encoded = user_input_answers(&questions, &answers);
        assert_eq!(encoded[0]["selectedLabel"], json!("Yes"));
        assert_eq!(encoded[1]["selectedLabels"], json!(["a", "b"]));
        assert_eq!(encoded[2]["freeText"], json!("free"));
    }

    #[test]
    fn muse_modes_map_to_closed_approval_vocabulary() {
        assert_eq!(approval_mode(RuntimeMode::Ask), "promptUnmatched");
        assert_eq!(
            approval_mode(RuntimeMode::AutoAcceptEdits),
            "promptUnmatched"
        );
        assert_eq!(approval_mode(RuntimeMode::Auto), "onRequest");
        assert_eq!(approval_mode(RuntimeMode::FullAccess), "allowAll");
    }

    #[test]
    fn muse_turn_outcome_uses_the_terminal_word() {
        assert_eq!(
            turn_outcome(&json!({"terminal": "completed"})),
            (true, None)
        );
        assert_eq!(
            turn_outcome(&json!({"terminal": "cancelled"})),
            (false, None)
        );
        let (success, summary) =
            turn_outcome(&json!({"terminal": "error", "error": {"message": "boom"}}));
        assert!(!success);
        assert_eq!(summary.as_deref(), Some("boom"));
    }

    /// The official `approval-round-trip` transcript (muse-code-sdk
    /// schema/msp/transcripts): every server `method` frame is replayed
    /// through the event translator exactly as the host would deliver it.
    #[test]
    fn muse_conformance_approval_round_trip() {
        const TRANSCRIPT: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/muse-approval-round-trip.ndjson"
        ));
        let (events, rx) = test_event_channel();
        let mut state = WorkerState {
            session_id: "0198f0aa-1111-7000-8000-0000000000aa".to_owned(),
            mode: RuntimeMode::Ask,
            eval: None,
            model: None,
            reasoning_effort: None,
            active_turn: None,
            finished_turns: Vec::new(),
            queued_turns: Vec::new(),
            retried_turns: HashSet::new(),
            items: HashMap::new(),
            approvals: HashMap::new(),
            user_inputs: HashMap::new(),
            subscription: MuseSubscription::detached(),
        };
        for line in TRANSCRIPT.lines() {
            let Ok(record) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if record.get("dir").and_then(Value::as_str) != Some("server") {
                continue;
            }
            let Some(frame) = record
                .get("raw")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            else {
                continue;
            };
            let Some(method) = frame.get("method").and_then(Value::as_str) else {
                continue;
            };
            handle_event(
                None,
                &events,
                &mut state,
                method,
                frame.get("params").unwrap_or(&Value::Null),
            );
        }

        let seen: Vec<DriverEvent> = rx.try_iter().collect();
        // turn/started then approval/requested then the approval/request
        // server-request (both reach the handler — the UI keys on the id).
        let permissions = seen
            .iter()
            .filter_map(|event| match event {
                DriverEvent::Permission {
                    request_id,
                    title,
                    options,
                    ..
                } => Some((request_id.clone(), title.clone(), options.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(permissions.len(), 2);
        let (request_id, title, options) = &permissions[0];
        assert_eq!(request_id, "0198f0ac-7777-7000-8000-0000000000e1");
        assert!(title.contains("Cargo.toml"));
        let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["allow_once", "allow_session", "abort"]);
        assert!(options[0].allow && options[1].allow && !options[2].allow);

        let tools = seen
            .iter()
            .filter_map(|event| match event {
                DriverEvent::RichActivity(item) => Some(item),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            tools
                .iter()
                .any(|item| { item.tool_name.as_deref() == Some("write_file") && item.complete })
        );
        assert!(
            seen.iter()
                .any(|event| matches!(event, DriverEvent::TurnFinished { success: true, .. }))
        );
        // approval/resolved cleared the pending map; the completed turn is a
        // known fork boundary.
        assert!(state.approvals.is_empty());
        let finished: Vec<(&str, bool)> = state
            .finished_turns
            .iter()
            .map(|turn| (turn.turn_id.as_str(), turn.completed))
            .collect();
        assert_eq!(finished, [("018f6a1e-9b3c-7c21-a54a-2f30bd3c9f10", true)]);
    }
}
