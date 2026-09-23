//! Provider switching on a started session. A different-provider pick on a
//! provider-locked session opens the confirmation dialog in
//! `provider_switch_dialog.rs`; confirming runs this pipeline:
//!
//! 1. The transcript the target provider has never seen — everything for a
//!    first-time target, or the delta past a suspended session's recorded
//!    boundary — becomes candidate context.
//! 2. A resumed target still receives the missed work verbatim in a
//!    `goddard-session-context` envelope. A fresh start instead receives
//!    the user's messages verbatim plus a compact per-turn index of
//!    representative spans Jev selects — the agent pulls a turn's
//!    authoritative text with `goddard-agent read` when it needs it.
//!    Extraction, not summarization, is what keeps the handoff lossless
//!    enough to trust.
//! 3. The envelope rides the next outbound prompt as a one-shot prepend
//!    (`AgentSession::take_provider_context`), the same delivery a
//!    worktree-move notice uses, so every provider receives it identically.
//!
//! A suspended provider's own conversation stays resumable: the switch
//! records its cursor and boundary so switching back later resumes it and
//! injects only the work it missed. When that cursor no longer resolves —
//! the provider-side session was deleted or expired — the switch falls back
//! to a fresh session seeded with the full extract, and the transcript
//! marker notes the restart.
//!
//! Degradation, in order: a daemon that cannot place `goddard-agent` on the
//! session's PATH (a remote host provisioned with the daemon alone) keeps
//! the verbatim push — the index would point at a tool the agent does not
//! have. A small segment skips the eval call and pushes verbatim anyway. A
//! missing eval backend or an answer set too thin to trust degrades the
//! index to a pointer-only handoff — the switch still proceeds.
//!
//! The eval call and the resumability probe run on the session's daemon via
//! `Command::Evaluate` / `Command::LoadProviderSession`, off the UI thread.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use uuid::Uuid;
use waku_protocol::eval::{EvalAnswer, EvalQuestion, Evaluation};
use waku_protocol::model::{
    ActivityItem, AgentSession, Message, MessageRole, ProviderKind, SuspendedProviderSession,
    TranscriptBoundary, TranscriptNotice, truncate_chars,
};

use super::*;

/// Decision-log `feature` for the compaction call.
const SWITCH_EVAL_FEATURE: &str = "provider-switch";
/// Compaction asks one question per transcript item over a large state — far
/// past the 5s budget latency-bound callers get. Still bounded: a wedged
/// backend must not hang the session forever.
const SWITCH_EVAL_TIMEOUT_SECS: u64 = 180;
/// A `Noul` probability at or above this keeps the item verbatim.
const KEEP_THRESHOLD: f64 = 0.5;
/// Per-item text cap so one giant tool output cannot dominate the state.
const ITEM_TEXT_CAP: usize = 2_000;
/// One index span's cap — a sentence or an activity line clipped to a few
/// lines of text.
const SPAN_TEXT_CAP: usize = 320;
/// The index lists at most this many scored spans under one turn — it is a
/// map of what to retrieve, not the transcript itself.
const INDEX_SPANS_PER_TURN: usize = 6;
/// Below this size a fresh-start segment is cheaper to push verbatim than
/// to index — and the eval call is skipped entirely.
const PUSH_FLOOR_BYTES: usize = 16 * 1024;
/// The serialized `items` array stays under this; the oldest scored items
/// drop first (always-kept user text never does).
const MAX_STATE_BYTES: usize = 256 * 1024;
/// Fewer answered questions than this fraction of those asked means the
/// backend's response cannot be trusted to select anything.
const MIN_ANSWERED_FRACTION: f64 = 0.5;

/// One transcript line offered to the evaluation model, carrying its
/// absolute position so segment membership survives list order.
struct ContextItem {
    /// Question key and envelope identity: `m12` for a message, `b4.1` for a
    /// block's activity.
    id: String,
    position: ItemPosition,
    /// User-authored text is always carried verbatim, never scored.
    always: bool,
    /// The 1-based turn the item belongs to — `goddard-agent read`'s `turn`
    /// argument selects by this number. Unturned content (markers, legacy
    /// rows) groups under `None`.
    turn: Option<usize>,
    text: String,
}

#[derive(Clone, Copy)]
enum ItemPosition {
    Message(usize),
    Block(usize),
}

impl ContextItem {
    /// Whether the item sits at or past `boundary` — the segment a suspended
    /// provider has never seen. Positions are append-order indexes, so a
    /// rewind that shortened the transcript simply matches fewer.
    fn in_segment(&self, boundary: TranscriptBoundary) -> bool {
        match self.position {
            ItemPosition::Message(index) => index >= boundary.messages,
            ItemPosition::Block(index) => index >= boundary.blocks,
        }
    }
}

/// The rendered text of one activity for the envelope — title plus the
/// bounded detail fields, clipped so a megabyte of tool output becomes a
/// page.
fn activity_text(activity: &ActivityItem) -> Option<String> {
    activity.condensed_text(ITEM_TEXT_CAP)
}

/// Flatten the transcript into scoreable items in reading order: blocks
/// render after `after_message` messages, so a block precedes the message at
/// that index. Hidden provider-facing nudges are not context. Each item
/// carries its 1-based turn so the index can point at
/// `goddard-agent read '{"turn": N}'`; unturned content (markers, legacy
/// rows) belongs to the turn it follows.
fn context_items(session: &AgentSession) -> Vec<ContextItem> {
    let turn_numbers: std::collections::HashMap<Uuid, usize> = session
        .turns
        .iter()
        .map(|entry| (entry.id, entry.turn_count))
        .collect();
    let turn_of = |turn_id: Option<Uuid>| turn_id.and_then(|id| turn_numbers.get(&id)).copied();
    let mut items = Vec::new();
    let mut last_turn = None;
    for position in 0..=session.messages.len() {
        for (block_index, block) in session.transcript_blocks.iter().enumerate() {
            if block.after_message != position {
                continue;
            }
            let block_turn = turn_of(block.turn_id).or(last_turn);
            for (activity_index, activity) in block.activities.iter().enumerate() {
                if let Some(text) = activity_text(activity) {
                    items.push(ContextItem {
                        id: format!("b{block_index}.{activity_index}"),
                        position: ItemPosition::Block(block_index),
                        always: false,
                        turn: block_turn,
                        text,
                    });
                }
            }
        }
        let Some(message) = session.messages.get(position) else {
            continue;
        };
        let message_turn = turn_of(message.turn_id).or(last_turn);
        if message_turn.is_some() {
            last_turn = message_turn;
        }
        if message.hidden || message.visible_content().trim().is_empty() {
            continue;
        }
        let role = match message.role {
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
            MessageRole::System => "System",
        };
        items.push(ContextItem {
            id: format!("m{position}"),
            position: ItemPosition::Message(position),
            always: message.role == MessageRole::User,
            turn: message_turn,
            // Compaction replays what the provider saw — the atoms' pasted
            // text and session tokens, not the bubble's chip labels.
            text: format!(
                "{role}: {}",
                composer::atom_payload_content(message.visible_content(), &message.atoms).trim()
            ),
        });
    }
    items
}

/// One verbatim fragment an index entry may carry: a line or sentence of an
/// assistant message, or one line of a condensed activity. Spans are what
/// Jev selects — never generated text — and each inherits its parent item's
/// turn for grouping.
struct IndexSpan {
    /// Question key: the parent item's id plus its own index, `m12.3`.
    id: String,
    text: String,
}

/// Split one scored item's text into span candidates: line by line, with
/// long lines broken at sentence boundaries so a dense paragraph still
/// offers quotable pieces.
fn item_spans(item: &ContextItem) -> Vec<IndexSpan> {
    let mut spans = Vec::new();
    let mut push = |text: &str| {
        let text = text.trim();
        if !text.is_empty() {
            spans.push(IndexSpan {
                id: format!("{}.{}", item.id, spans.len()),
                text: truncate_chars(text, SPAN_TEXT_CAP),
            });
        }
    };
    for line in item.text.lines() {
        if line.trim().chars().count() <= SPAN_TEXT_CAP {
            push(line);
        } else {
            for piece in line.split_inclusive(['.', '!', '?']) {
                push(piece);
            }
        }
    }
    spans
}

/// The eval `state`/`questions` pair for the index's span candidates —
/// same shape as [`compaction_eval`], one `Noul` per span.
fn span_eval(spans: &[IndexSpan]) -> (Value, BTreeMap<String, EvalQuestion>) {
    let mut size = spans.iter().map(|span| span.text.len() + 64).sum::<usize>();
    let mut kept: Vec<&IndexSpan> = spans.iter().collect();
    // The serialized state stays bounded the same way: oldest spans drop.
    let mut oldest = 0usize;
    while size > MAX_STATE_BYTES && oldest < kept.len() {
        size -= kept.remove(oldest).text.len() + 64;
        oldest += 1;
    }
    let state = json!({
        "task": "session-context-index",
        "items": kept
            .iter()
            .map(|span| json!({ "id": span.id, "text": span.text }))
            .collect::<Vec<_>>(),
    });
    let questions = kept
        .iter()
        .map(|span| {
            (
                span.id.clone(),
                EvalQuestion::Noul {
                    instructions: "This span is from an agent coding session migrating to a \
                        different provider. The new agent starts fresh but can pull any turn's \
                        full transcript on demand. Answer true when the span is worth indexing \
                        as a pointer: a decision, requirement, file change and its path, an \
                        error and how it was resolved, a command's outcome, or a fact the new \
                        agent cannot re-derive from the repository. Answer false for \
                        acknowledgements, pleasantries, and superseded attempts."
                        .to_owned(),
                    criteria: None,
                },
            )
        })
        .collect();
    (state, questions)
}

/// The eval `state`/`questions` pair for one segment. When the serialized
/// items would exceed [`MAX_STATE_BYTES`], the oldest scored items drop out
/// first — the newest context and every user message are the last to go.
fn compaction_eval(items: &[ContextItem]) -> (Value, BTreeMap<String, EvalQuestion>) {
    let mut kept: Vec<&ContextItem> = items.iter().collect();
    let mut size = kept.iter().map(|item| item.text.len() + 64).sum::<usize>();
    let mut oldest = 0usize;
    while size > MAX_STATE_BYTES {
        while oldest < kept.len() && kept[oldest].always {
            oldest += 1;
        }
        if oldest >= kept.len() {
            break;
        }
        size -= kept.remove(oldest).text.len() + 64;
    }
    let state = json!({
        "task": "session-context-extraction",
        "items": kept
            .iter()
            .map(|item| json!({ "id": item.id, "text": item.text }))
            .collect::<Vec<_>>(),
    });
    let questions = kept
        .iter()
        .filter(|item| !item.always)
        .map(|item| {
            (
                item.id.clone(),
                EvalQuestion::Noul {
                    instructions: "This item is from an agent coding session being migrated to \
                        a different provider. Answer true when the new agent must see it \
                        verbatim to continue correctly: requirements and decisions, file edits \
                        and their paths, errors and how they were resolved, command outcomes, \
                        or facts that cannot be re-derived from the repository. Answer false \
                        for acknowledgements, pleasantries, and superseded attempts."
                        .to_owned(),
                    criteria: None,
                },
            )
        })
        .collect();
    (state, questions)
}

/// What the switch produces once the daemon work finishes: the envelope to
/// prepend to the next prompt, and how the target's provider-side state was
/// reached.
struct SwitchOutcome {
    envelope: String,
    /// The target had a suspended session whose cursor still resolved — it
    /// resumes and receives the delta. `false` means a fresh session gets
    /// the full extract; with `was_suspended` set, that is a restart.
    resumed: bool,
    was_suspended: bool,
}

impl SwitchOutcome {
    /// A suspended session that could not resume: the marker says so.
    fn restarted(&self) -> bool {
        !self.resumed && self.was_suspended
    }
}

/// A `Noul` response that answered so few questions its selections cannot be
/// trusted — the handoff degrades rather than shipping an arbitrary subset.
fn answers_too_thin(asked: usize, evaluation: &Evaluation) -> bool {
    asked > 0 && (evaluation.answers.len() as f64) < asked as f64 * MIN_ANSWERED_FRACTION
}

/// The verbatim push envelope: every always-kept item plus each scored item
/// Jev kept, in transcript order. `evaluation: None` pushes the segment
/// unfiltered — the degradation path when nothing could score it.
fn assemble_push_envelope(
    segment: &[ContextItem],
    resumed: bool,
    from: ProviderKind,
    evaluation: Option<&Evaluation>,
) -> String {
    let kept: Vec<&str> = segment
        .iter()
        .filter(|item| {
            item.always
                || match evaluation.and_then(|evaluation| evaluation.answers.get(&item.id)) {
                    Some(EvalAnswer::Noul { noul }) => *noul >= KEEP_THRESHOLD,
                    // An unanswered or mistyped item errs toward retention.
                    _ => true,
                }
        })
        .map(|item| item.text.as_str())
        .collect();
    let intro = if resumed {
        format!(
            "This task ran on {} while you were away. The work it did in the meantime \
             follows verbatim; your own earlier context is still intact. Treat it as \
             established history and continue the task.",
            from.display_name()
        )
    } else {
        format!(
            "This task was migrated to you from {}. The earlier conversation is not in \
             your context — the essentials, extracted verbatim from its transcript, \
             follow. Treat them as established history and continue the task from its \
             current state.",
            from.display_name()
        )
    };
    let kind = if resumed { "delta" } else { "full" };
    format!(
        "{intro}\n\n<goddard-session-context source=\"{}\" kind=\"{kind}\">\n{}\n</goddard-session-context>",
        from.id(),
        kept.join("\n\n")
    )
}

/// Turn-grouped index lines for one segment: `lines_of` yields each item's
/// `(line, scored)` pairs — scored lines cap at [`INDEX_SPANS_PER_TURN`]
/// per turn, always-kept user text never does. Returns the rendered body
/// plus the `turns="low-high"` attribute for the envelope tag.
fn index_body(
    segment: &[ContextItem],
    lines_of: impl Fn(&ContextItem) -> Vec<(String, bool)>,
) -> (String, String) {
    let mut groups: Vec<(Option<usize>, Vec<String>, usize)> = Vec::new();
    let group = |groups: &mut Vec<(Option<usize>, Vec<String>, usize)>, turn: Option<usize>| {
        groups
            .iter()
            .position(|(group_turn, ..)| *group_turn == turn)
            .unwrap_or_else(|| {
                groups.push((turn, Vec::new(), 0));
                groups.len() - 1
            })
    };
    for item in segment {
        for (line, scored_line) in lines_of(item) {
            let index = group(&mut groups, item.turn);
            let (.., lines, scored) = &mut groups[index];
            if !scored_line || *scored < INDEX_SPANS_PER_TURN {
                *scored += usize::from(scored_line);
                lines.push(line);
            }
        }
    }
    let mut body = String::new();
    for (turn, lines, ..) in &groups {
        // Unturned content — markers, legacy rows — has no `read` address;
        // it lists plainly rather than pointing at a turn that is not its.
        if let Some(turn) = turn {
            body.push_str(&format!("turn {turn}\n"));
        }
        for line in lines {
            body.push_str("  ");
            body.push_str(line);
            body.push('\n');
        }
        body.push('\n');
    }
    let range = match (
        groups.iter().filter_map(|(turn, ..)| *turn).min(),
        groups.iter().filter_map(|(turn, ..)| *turn).max(),
    ) {
        (Some(low), Some(high)) if low == high => format!(" turns=\"{low}\""),
        (Some(low), Some(high)) => format!(" turns=\"{low}-{high}\""),
        _ => String::new(),
    };
    (body, range)
}

/// The pull-model handoff: the user's own messages verbatim plus a per-turn
/// index of the spans Jev kept, and the exact `goddard-agent read`
/// invocations that pull a turn's authoritative text. `evaluation: None`
/// produces the pointer-only degradation — the same skeleton with no scored
/// spans.
fn assemble_index_envelope(
    segment: &[ContextItem],
    resumed: bool,
    from: ProviderKind,
    evaluation: Option<&Evaluation>,
) -> String {
    let (body, range) = index_body(segment, |item| {
        if item.always {
            return vec![(item.text.clone(), false)];
        }
        let Some(evaluation) = evaluation else {
            return Vec::new();
        };
        item_spans(item)
            .into_iter()
            .filter(|span| {
                matches!(
                    evaluation.answers.get(&span.id),
                    Some(EvalAnswer::Noul { noul }) if *noul >= KEEP_THRESHOLD
                )
            })
            .map(|span| (format!("— {}", span.text), true))
            .collect()
    });
    let intro = if resumed {
        format!(
            "This task ran on {} while you were away. The turns it added are indexed \
             below — read their full text with `goddard-agent read '{{\"turn\": N}}'` \
             before relying on details; your own earlier context is still intact.",
            from.display_name()
        )
    } else {
        format!(
            "This task was migrated to you from {}. Its earlier conversation is not in \
             your context — Goddard keeps it. `goddard-agent read '{{}}'` returns the \
             transcript listing; `goddard-agent read '{{\"turn\": N}}'` returns one \
             turn's full messages and tool output. Below are the user's own words \
             verbatim and an index of verbatim excerpts by turn — read a turn before \
             relying on its details.",
            from.display_name()
        )
    };
    format!(
        "{intro}\n\n<goddard-session-context source=\"{}\" kind=\"index\"{range}>\n{}</goddard-session-context>",
        from.id(),
        body
    )
}

/// A picker pick routed through the switch flow: explicit model when the row
/// named one, else `None` and the switch resolves the provider's MRU.
pub(super) struct ProviderSwitchPick {
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub fast: bool,
}

/// Estimate shown in the confirm dialog: how many turns the handoff covers,
/// counting only the segment the target has never seen.
pub(super) fn provider_switch_estimate(session: &AgentSession, target: ProviderKind) -> usize {
    let boundary = session
        .suspended_provider_sessions
        .iter()
        .find(|entry| entry.provider == target)
        .map(|entry| entry.boundary)
        .unwrap_or_default();
    let items = context_items(session);
    let mut turns = items
        .iter()
        .filter(|item| item.in_segment(boundary))
        .filter_map(|item| item.turn)
        .collect::<std::collections::BTreeSet<_>>();
    // Unturned items (markers, legacy rows) count as one group.
    if items
        .iter()
        .any(|item| item.in_segment(boundary) && item.turn.is_none())
    {
        turns.insert(usize::MAX);
    }
    turns.len()
}

impl Waku {
    /// The newest sibling session's model on `provider` — the MRU a switch
    /// lands on when the pick named no model. Traits then come from the
    /// per-model memory like any other pick.
    fn mru_model_for(&self, provider: ProviderKind) -> Option<String> {
        self.state
            .sessions
            .iter()
            .filter(|session| session.provider == provider)
            .filter_map(|session| session.model.as_deref().map(|m| (m, session.updated_at)))
            .max_by_key(|(_, updated_at)| *updated_at)
            .map(|(model, _)| model.to_owned())
    }

    /// Confirm the dialog: snapshot what the background work needs, then run
    /// the resumability probe and the compaction eval off the UI thread.
    /// Failure aborts — the session stays on its current provider.
    pub(super) fn confirm_provider_switch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.provider_switch_dialog.take() else {
            return;
        };
        let session_id = dialog.session_id;
        let target = dialog.pick.provider;
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            self.refocus_composer(window, cx);
            return;
        };
        // The dialog sat open; the session may have moved since.
        if session.is_busy()
            || session.provider == target
            || !session.detail_loaded
            || self.provider_switch_in_flight.contains(&session_id)
        {
            self.refocus_composer(window, cx);
            return;
        }
        let Some(daemon) = self.daemon_for_session(session_id) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            self.refocus_composer(window, cx);
            return;
        };
        // Neither the eval backend nor the read surface is required to
        // switch: a missing backend degrades the index to a pointer, and a
        // daemon without `goddard-agent` falls back to the verbatim push.
        let eval_available = daemon
            .settings()
            .eval
            .is_some_and(|eval| !eval.credential_missing());
        let client = daemon.client();
        let cli_available = client.agent_cli_available();
        let suspended = session
            .suspended_provider_sessions
            .iter()
            .find(|entry| entry.provider == target)
            .cloned();
        // A suspended session is resumed only when its provider-side record
        // still loads; the probe is the resumability check.
        let key = self.daemons.session_owner(session_id);
        let cwd = self
            .workspace_path_for_session(session)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let history_probe = suspended
            .clone()
            .map(|entry| self.store.provider_session_history(key, entry.cursor, cwd));
        let items = context_items(session);
        let from = session.provider;
        let was_suspended = suspended.is_some();
        self.provider_switch_in_flight.insert(session_id);
        self.show_toast(tr!(
            "provider_switch.working",
            provider = target.display_name()
        ));
        cx.notify();
        let pick = dialog.pick;
        cx.spawn(async move |waku, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let resumed = match history_probe {
                        Some(probe) => probe().is_ok(),
                        None => false,
                    };
                    let boundary = if resumed {
                        suspended.map(|entry| entry.boundary).unwrap_or_default()
                    } else {
                        TranscriptBoundary::default()
                    };
                    let segment: Vec<ContextItem> = items
                        .into_iter()
                        .filter(|item| item.in_segment(boundary))
                        .collect();
                    let small = segment.iter().map(|item| item.text.len()).sum::<usize>()
                        <= PUSH_FLOOR_BYTES;
                    // Which handoff the segment gets:
                    // - no `goddard-agent` on the daemon's host: verbatim push,
                    //   filtered when the eval ran — the index would point at
                    //   a tool the agent does not have;
                    // - resumed: the verbatim delta, same as always;
                    // - fresh + small: the verbatim push, no eval needed;
                    // - fresh + large: the index of Jev-selected spans.
                    let use_index = !resumed && cli_available && !small;
                    let evaluate = eval_available && (resumed || !small);
                    let (state, questions) = if !evaluate {
                        (json!({}), BTreeMap::new())
                    } else if use_index {
                        let spans: Vec<IndexSpan> = segment
                            .iter()
                            .filter(|item| !item.always)
                            .flat_map(item_spans)
                            .collect();
                        span_eval(&spans)
                    } else {
                        compaction_eval(&segment)
                    };
                    let asked = questions.len();
                    // An eval failure — no backend, a transport error, an
                    // unparseable reply — degrades to the pointer/unfiltered
                    // handoff rather than aborting the switch.
                    let evaluation = if evaluate {
                        client
                            .request(
                                Uuid::nil(),
                                session_id,
                                waku_client::Command::Evaluate {
                                    state,
                                    questions,
                                    feature: Some(SWITCH_EVAL_FEATURE.to_owned()),
                                    timeout_secs: Some(SWITCH_EVAL_TIMEOUT_SECS),
                                },
                            )
                            .ok()
                            .and_then(|payload| match payload {
                                waku_client::ResponsePayload::Evaluation { evaluation } => {
                                    Some(evaluation)
                                }
                                _ => None,
                            })
                    } else {
                        None
                    };
                    // An answer set too thin to trust counts as no answers —
                    // pointer-only when the agent can read, unfiltered push
                    // when it cannot.
                    let evaluation = evaluation.filter(|eval| !answers_too_thin(asked, eval));
                    let envelope = if !cli_available {
                        assemble_push_envelope(&segment, resumed, from, evaluation.as_ref())
                    } else if resumed {
                        match evaluation {
                            Some(evaluation) => {
                                assemble_push_envelope(&segment, true, from, Some(&evaluation))
                            }
                            // Nothing usable scored the delta — the resumed
                            // agent reads it back itself.
                            None => assemble_index_envelope(&segment, true, from, None),
                        }
                    } else if small {
                        assemble_push_envelope(&segment, false, from, None)
                    } else {
                        assemble_index_envelope(&segment, false, from, evaluation.as_ref())
                    };
                    anyhow::Ok(SwitchOutcome {
                        envelope,
                        resumed,
                        was_suspended,
                    })
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_provider_switch(session_id, pick, outcome, cx);
            });
        })
        .detach();
    }

    /// Apply a completed switch: suspend the old provider's conversation,
    /// point the session at the target, stage the envelope for the next
    /// prompt, and mark the transcript. On failure nothing has moved.
    fn finish_provider_switch(
        &mut self,
        session_id: Uuid,
        pick: ProviderSwitchPick,
        outcome: anyhow::Result<SwitchOutcome>,
        cx: &mut Context<Self>,
    ) {
        self.provider_switch_in_flight.remove(&session_id);
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                self.show_toast(tr!("provider_switch.failed", error = format!("{error:#}")));
                cx.notify();
                return;
            }
        };
        // Resolve the model before borrowing the session mutably: the pick's
        // model when the row named one, else the target provider's MRU, else
        // `None` and `session_options` resolves the provider's default.
        let (model, effort, service_tier, context_window) = match pick.model.clone() {
            Some(model) => {
                let (_, _, window) = self.state.model_traits_for(pick.provider, &model);
                (
                    Some(model),
                    pick.effort.clone(),
                    pick.fast.then(|| "fast".to_owned()),
                    window,
                )
            }
            None => match self.mru_model_for(pick.provider) {
                Some(model) => {
                    let (effort, tier, window) = self.state.model_traits_for(pick.provider, &model);
                    (Some(model), effort, tier, window)
                }
                None => (None, None, None, None),
            },
        };
        let Some(session) = self.state.session_mut(session_id) else {
            return;
        };
        if session.provider == pick.provider {
            return;
        }
        let from = session.provider;
        // Suspend the provider being left — only a live provider-side
        // conversation is worth recording. The boundary precedes the marker
        // below so the marker lands in the delta a return visit compacts.
        if let Some(cursor) = session.provider_cursor.take() {
            let boundary = session.transcript_boundary();
            match session
                .suspended_provider_sessions
                .iter_mut()
                .find(|entry| entry.provider == from)
            {
                Some(entry) => {
                    entry.cursor = cursor;
                    entry.boundary = boundary;
                }
                None => session
                    .suspended_provider_sessions
                    .push(SuspendedProviderSession {
                        provider: from,
                        cursor,
                        boundary,
                    }),
            }
        }
        // Adopt the target's suspended conversation when it resumed; a
        // restart drops the stale cursor entirely.
        let resumed_cursor = if outcome.resumed {
            let position = session
                .suspended_provider_sessions
                .iter()
                .position(|entry| entry.provider == pick.provider);
            position.map(|index| session.suspended_provider_sessions.remove(index).cursor)
        } else {
            session
                .suspended_provider_sessions
                .retain(|entry| entry.provider != pick.provider);
            None
        };
        session.provider = pick.provider;
        session.provider_cursor = resumed_cursor;
        session.provider_session_id = None;
        session.model = model;
        session.reasoning_effort = effort;
        session.service_tier = service_tier;
        session.context_window = context_window;
        session.agent_preset = None;
        session.auto_route = false;
        session.route_decision = None;
        // These belong to the previous provider's process.
        session.available_commands.clear();
        session.context_usage = None;
        session.thread_goal = None;
        let restarted = outcome.restarted();
        session.pending_provider_context = Some(outcome.envelope);
        let mut marker = Message::new(
            MessageRole::System,
            if restarted {
                tr!(
                    "transcript.provider_switched_restarted",
                    from = from.display_name(),
                    to = pick.provider.display_name()
                )
            } else {
                tr!(
                    "transcript.provider_switched",
                    from = from.display_name(),
                    to = pick.provider.display_name()
                )
            },
        );
        marker.notice = Some(TranscriptNotice::ProviderSwitched {
            from,
            to: pick.provider,
            restarted,
        });
        session.messages.push(marker);
        session.updated_at = unix_time();
        self.reset_session_runtime(session_id);
        self.refresh_composer_sources(cx);
        // A switch is a model choice too — mirror the resolved combo into
        // the last-used defaults like `choose_model` does.
        if let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            && let Some(model) = session.model.clone()
        {
            self.state.last_provider = pick.provider;
            self.state.last_auto_route = false;
            self.state.last_model = Some(model);
            self.state
                .last_reasoning_effort
                .clone_from(&session.reasoning_effort);
            self.state
                .last_service_tier
                .clone_from(&session.service_tier);
            self.state
                .last_context_window
                .clone_from(&session.context_window);
        }
        self.save();
        cx.notify();
        let focus = self.composer_focus(cx);
        let window_handle = self.window_handle;
        let _ = window_handle.update(cx, |_, window, cx| window.focus(&focus, cx));
        // A prompt submitted mid-switch queued as a follow-up; it belongs to
        // the new provider now.
        self.drain_queued_message(session_id, cx);
    }

    /// Close any switch dialog and put the composer back in focus.
    pub(super) fn refocus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::eval::EvalAnswer;

    fn session_with_history() -> AgentSession {
        let project = waku_protocol::model::Project::from_path(std::path::PathBuf::from("/tmp/p"));
        let mut session = AgentSession::new(project.id, ProviderKind::Claude);
        session.push_message(MessageRole::User, "fix the build");
        session.push_message(MessageRole::Assistant, "done");
        session
            .transcript_blocks
            .push(waku_protocol::model::TranscriptBlock {
                after_message: 2,
                turn_id: None,
                activities: vec![ActivityItem::new(
                    None,
                    waku_protocol::model::ActivityKind::Command,
                    "Ran cargo build",
                    Some("ok".to_owned()),
                    true,
                )],
            });
        session
    }

    fn evaluation(pairs: &[(&str, f64)]) -> Evaluation {
        Evaluation {
            model: "jev-test".to_owned(),
            answers: pairs
                .iter()
                .map(|(id, noul)| ((*id).to_owned(), EvalAnswer::Noul { noul: *noul }))
                .collect(),
            usage: Default::default(),
            latency_ms: 0,
            provider_metadata: None,
        }
    }

    #[test]
    fn items_carry_absolute_positions() {
        let session = session_with_history();
        let items = context_items(&session);
        assert_eq!(items.len(), 3);
        assert!(items[0].always);
        assert_eq!(items[0].id, "m0");
        // The block anchored after message 2 renders after both messages.
        assert_eq!(items[2].id, "b0.0");
    }

    #[test]
    fn delta_segment_keeps_only_post_boundary_items() {
        let session = session_with_history();
        let items = context_items(&session);
        let boundary = TranscriptBoundary {
            messages: 1,
            blocks: 0,
        };
        let segment: Vec<_> = items
            .iter()
            .filter(|item| item.in_segment(boundary))
            .collect();
        assert_eq!(segment.len(), 2);
        assert_eq!(segment[0].id, "m1");
    }

    #[test]
    fn push_envelope_keeps_user_text_and_selected_items_verbatim() {
        let session = session_with_history();
        let items = context_items(&session);
        let evaluation = evaluation(&[("m1", 0.9), ("b0.0", 0.1)]);
        let envelope =
            assemble_push_envelope(&items, false, ProviderKind::Claude, Some(&evaluation));
        assert!(envelope.contains("User: fix the build"));
        assert!(envelope.contains("Assistant: done"));
        assert!(!envelope.contains("Ran cargo build"));
        assert!(envelope.contains("kind=\"full\""));

        // No evaluation pushes the segment unfiltered.
        let envelope = assemble_push_envelope(&items, false, ProviderKind::Claude, None);
        assert!(envelope.contains("Ran cargo build"));
    }

    #[test]
    fn a_thin_answer_set_is_detected_not_fatal() {
        let session = session_with_history();
        let items = context_items(&session);
        let asked = items.iter().filter(|item| !item.always).count();
        assert!(answers_too_thin(asked, &evaluation(&[])));
        assert!(!answers_too_thin(
            asked,
            &evaluation(&[("m1", 0.9), ("b0.0", 0.2)])
        ));
    }

    #[test]
    fn the_index_envelope_groups_verbatim_spans_by_turn() {
        let project = waku_protocol::model::Project::from_path(std::path::PathBuf::from("/tmp/p"));
        let mut session = AgentSession::new(project.id, ProviderKind::Claude);
        session.begin_turn("fix the build");
        // One long line — the sentence splitter offers its pieces as
        // separate spans.
        session.push_message(
            MessageRole::Assistant,
            format!(
                "The lock ordering inverts in sync(). {}",
                "noise ".repeat(80)
            ),
        );
        session.finish_active_turn(waku_protocol::model::TurnStatus::Completed);
        session.begin_turn("and the tests?");
        session.push_message(MessageRole::Assistant, "All green now.");
        session.finish_active_turn(waku_protocol::model::TurnStatus::Completed);

        let items = context_items(&session);
        // Turn one keeps its first assistant sentence; the filler tail and
        // turn two's reply drop.
        let evaluation = evaluation(&[("m1.0", 0.9), ("m1.1", 0.1), ("m3.0", 0.1)]);
        let envelope =
            assemble_index_envelope(&items, false, ProviderKind::Claude, Some(&evaluation));

        assert!(envelope.contains("kind=\"index\""));
        assert!(envelope.contains("turns=\"1-2\""));
        assert!(envelope.contains("turn 1\n  User: fix the build"));
        assert!(envelope.contains("turn 2\n  User: and the tests?"));
        assert!(envelope.contains("The lock ordering inverts in sync()."));
        assert!(!envelope.contains("noise"));
        assert!(!envelope.contains("All green"));
        // The pull contract is named exactly.
        assert!(envelope.contains("goddard-agent read '{\"turn\": N}'"));

        // No evaluation is the pointer-only degradation: user text and the
        // turn map remain, nothing else is claimed.
        let envelope = assemble_index_envelope(&items, false, ProviderKind::Claude, None);
        assert!(envelope.contains("kind=\"index\""));
        assert!(envelope.contains("User: fix the build"));
        assert!(!envelope.contains("lock ordering"));
    }

    #[test]
    fn a_span_kept_past_the_turn_cap_still_names_its_turn() {
        let project = waku_protocol::model::Project::from_path(std::path::PathBuf::from("/tmp/p"));
        let mut session = AgentSession::new(project.id, ProviderKind::Claude);
        session.begin_turn("go");
        session.push_message(
            MessageRole::Assistant,
            (0..10)
                .map(|index| format!("detail {index}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        session.finish_active_turn(waku_protocol::model::TurnStatus::Completed);

        let items = context_items(&session);
        // Every span kept — the per-turn cap bounds what the index lists.
        let pairs: Vec<(String, f64)> = (0..10).map(|index| (format!("m1.{index}"), 0.9)).collect();
        let evaluation = evaluation(
            &pairs
                .iter()
                .map(|(id, noul)| (id.as_str(), *noul))
                .collect::<Vec<_>>(),
        );
        let envelope =
            assemble_index_envelope(&items, false, ProviderKind::Claude, Some(&evaluation));
        let kept = (0..10)
            .filter(|index| envelope.contains(&format!("detail {index}")))
            .count();
        assert_eq!(kept, INDEX_SPANS_PER_TURN);
        assert!(envelope.contains("turn 1\n"));
    }

    #[test]
    fn oversized_state_drops_oldest_scored_items_first() {
        let project = waku_protocol::model::Project::from_path(std::path::PathBuf::from("/tmp/p"));
        let mut session = AgentSession::new(project.id, ProviderKind::Claude);
        session.push_message(MessageRole::User, "task");
        for _ in 0..300 {
            session.push_message(MessageRole::Assistant, "x".repeat(2000));
        }
        let items = context_items(&session);
        let (state, questions) = compaction_eval(&items);
        let serialized = serde_json::to_string(&state).unwrap();
        assert!(serialized.len() <= MAX_STATE_BYTES + 1024);
        // The newest scored items and the user message survive; the oldest
        // scored items are what drop.
        assert!(questions.contains_key("m300"));
        assert!(!questions.contains_key("m1"));
        assert!(serialized.contains("User: task"));
    }
}
