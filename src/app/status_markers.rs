//! Turn status markers: while the experiment is on, each assistant turn that
//! ends naturally is scored by the daemon's evaluation model — one `Choice`
//! question picks how the turn ended (complete, awaiting input, partial,
//! blocked, failed, other). A second choice clarifies what input is needed
//! when the turn awaits the user, while independent Nouls flag qualities that can
//! co-occur with any ending (unverified, drifted). Cleared markers render as
//! chips on the response footer: colored icon and name, dim confidence
//! percent.
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
use waku_protocol::model::{ActivityKind, AgentSession, MessageRole, TurnStatus};

use super::*;

/// One status the evaluation model scores each settled turn against. `id` is
/// the question key or choice option and the decision-log marker name. Ending
/// markers are options of one `Choice` question — `threshold` is the bar the
/// winner's probability must clear — while flag markers are independent
/// Nouls and `threshold` is the probability each must reach.
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

/// How the turn ended — the options of one `Choice` question that compete
/// for probability mass, so only the dominant ending can render and endings
/// that contradict each other never share the footer. `other` is the escape
/// bucket: when it wins, no ending chip renders. Bad endings carry lower
/// thresholds — a false chip is cheap next to a missed failure — while
/// `complete` only renders when the model is sure.
const ENDING_QUESTION: &str = "ending";
const ENDING_OTHER_OPTION: &str = "other";
const INPUT_QUESTION: &str = "awaiting-input-kind";
const INPUT_OTHER_OPTION: &str = "other";
const ENDING_MARKERS: &[StatusMarker] = &[
    StatusMarker {
        id: "complete",
        label_key: "status_markers.complete",
        icon: "icons/check.svg",
        tone: MarkerTone::Success,
        threshold: 0.65,
        instructions: "The requested work was carried out end to end and the final \
            response reports it done — judge the ask as the `prompt` plus any \
            `priorPrompts` still in play.",
    },
    StatusMarker {
        id: "awaiting-input",
        label_key: "status_markers.awaiting_input",
        icon: "icons/chat.svg",
        tone: MarkerTone::Info,
        threshold: 0.55,
        instructions: "The requested work remains unfinished because the assistant \
            asks the user for a go-ahead, a decision, or missing details before \
            continuing. A rhetorical question after completed work does not count.",
    },
    StatusMarker {
        id: "partial",
        label_key: "status_markers.partial",
        icon: "icons/hourglass.svg",
        tone: MarkerTone::Warning,
        threshold: 0.50,
        instructions: "The turn stopped before the work was finished — cut off by a \
            context or step limit (`contextUsage` near its `window` is direct \
            evidence), truncated output, or an explicit promise to continue later.",
    },
    StatusMarker {
        id: "blocked",
        label_key: "status_markers.blocked",
        icon: "icons/block.svg",
        tone: MarkerTone::Danger,
        threshold: 0.45,
        instructions: "The assistant cannot proceed without something outside its \
            control — a missing permission, credential, file, tool, or environment \
            resource; `toolErrors` output tails surface those failures.",
    },
    StatusMarker {
        id: "failed",
        label_key: "status_markers.failed",
        icon: "icons/x-bold.svg",
        tone: MarkerTone::Danger,
        threshold: 0.45,
        instructions: "The turn failed — a reported error, a tool failure that ended \
            the work, or the assistant saying it could not complete the request; \
            `toolErrors` lists the calls that went wrong.",
    },
];

/// Only one input subtype can replace the generic awaiting-input chip. Keep
/// the generic chip when the subtype is ambiguous or its score is weak.
const INPUT_MARKERS: &[StatusMarker] = &[
    StatusMarker {
        id: "go-ahead",
        label_key: "status_markers.go_ahead",
        icon: "icons/chat.svg",
        tone: MarkerTone::Info,
        threshold: 0.55,
        instructions: "The assistant asks whether it should proceed with proposed \
            work or requests the user's approval before taking a concrete next \
            action. Examples include 'Want me to proceed?' and 'May I deploy it?'",
    },
    StatusMarker {
        id: "decision",
        label_key: "status_markers.decision",
        icon: "icons/chat.svg",
        tone: MarkerTone::Info,
        threshold: 0.55,
        instructions: "The assistant presents two or more viable options and \
            needs the user to choose one before continuing. A yes/no question \
            about whether to proceed is a go-ahead instead.",
    },
    StatusMarker {
        id: "details",
        label_key: "status_markers.details",
        icon: "icons/chat.svg",
        tone: MarkerTone::Info,
        threshold: 0.55,
        instructions: "The assistant needs missing facts, requirements, files, or \
            clarification from the user before continuing. A request to approve \
            an already specified action is a go-ahead instead.",
    },
];

/// A user action justified by the latest settled turn's input marker.
/// Decision labels come only from short, explicitly enumerated options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum StatusSuggestedAction {
    Proceed,
    Choose { option: String },
    AddDetails,
    ChooseManually,
}

fn explicit_decision_options(response: &str) -> Vec<String> {
    let mut run: Vec<(char, String)> = Vec::new();
    let mut latest = Vec::new();
    for line in response
        .lines()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let line = line.trim();
        let option = line
            .split_once(". ")
            .filter(|(index, _)| index.len() == 1)
            .and_then(|(index, title)| {
                let index = index.chars().next()?;
                (index.is_ascii_digit() || ('A'..='D').contains(&index)).then_some((index, title))
            });
        if let Some((index, title)) = option {
            let title = title.trim();
            let title = title
                .strip_prefix("**")
                .and_then(|bold| bold.split_once("**").map(|(title, _)| title))
                .unwrap_or(title)
                .trim_matches('`')
                .trim();
            if !title.is_empty() && title.chars().count() <= 42 {
                if run.is_empty() && index != '1' && index != 'A' {
                    continue;
                }
                if run.last().is_some_and(|(last, _)| {
                    index as u32 != *last as u32 + 1
                        || index.is_ascii_digit() != last.is_ascii_digit()
                }) {
                    if (2..=3).contains(&run.len()) {
                        latest = std::mem::take(&mut run);
                    } else {
                        run.clear();
                    }
                }
                run.push((index, title.to_owned()));
                continue;
            }
        }
        if (2..=3).contains(&run.len()) {
            latest = std::mem::take(&mut run);
        } else {
            run.clear();
        }
    }
    if (2..=3).contains(&run.len()) {
        latest = run;
    }
    latest
        .into_iter()
        .map(|(index, title)| format!("{index}. {title}"))
        .collect()
}

fn suggested_actions(evaluation: &Evaluation, response: &str) -> Vec<StatusSuggestedAction> {
    let marker = cleared_markers(evaluation)
        .into_iter()
        .find(|(marker, _)| matches!(marker.id, "go-ahead" | "decision" | "details"));
    match marker.map(|(marker, _)| marker.id) {
        Some("go-ahead") => vec![StatusSuggestedAction::Proceed],
        Some("details") => vec![StatusSuggestedAction::AddDetails],
        Some("decision") => {
            let options = explicit_decision_options(response);
            if options.is_empty() {
                vec![StatusSuggestedAction::ChooseManually]
            } else {
                options
                    .into_iter()
                    .map(|option| StatusSuggestedAction::Choose { option })
                    .collect()
            }
        }
        _ => Vec::new(),
    }
}

/// Flags stay independent Nouls — qualities that legitimately co-occur with
/// any ending (a turn can be `complete` *and* `unverified`, or `blocked`
/// *and* `drifted`), so they must not compete for the ending's probability
/// mass.
const FLAG_MARKERS: &[StatusMarker] = &[
    StatusMarker {
        id: "unverified",
        label_key: "status_markers.unverified",
        icon: "icons/eye.svg",
        tone: MarkerTone::Warning,
        threshold: 0.75,
        instructions: "Did the turn finish without its work being verified — code it \
            changed but never built, tested, or ran, claims it did not check, or the \
            last build/test/run in `toolSequence` ended `(failed)` and the turn closed \
            without a passing rerun? `toolErrors` carries the failing output tails.",
    },
    StatusMarker {
        id: "drifted",
        label_key: "status_markers.drifted",
        icon: "icons/target.svg",
        tone: MarkerTone::Favorite,
        threshold: 0.70,
        instructions: "Did the assistant do work outside the scope of what the user \
            asked — unrelated edits, a different task, or unrequested scope creep? \
            Judge scope against the `prompt` and `priorPrompts` together; asks from \
            earlier turns in the same session are in scope.",
    },
    StatusMarker {
        id: "needs-review",
        label_key: "status_markers.needs_review",
        icon: "icons/file-diff.svg",
        tone: MarkerTone::Warning,
        threshold: 0.70,
        instructions: "Did the turn touch surface a human should review even when it \
            works — authentication, credentials or secret files, migrations, CI or \
            release configuration, destructive commands? `filesChanged` lists what \
            changed with per-file diff size.",
    },
    StatusMarker {
        id: "thrash",
        label_key: "status_markers.thrash",
        icon: "icons/rotate-cw.svg",
        tone: MarkerTone::Warning,
        threshold: 0.70,
        instructions: "Did the assistant retry the same failing approach repeatedly — \
            `toolSequence` shows repeated `(failed)` steps or a `×N` run of identical \
            calls? A couple of retries is normal debugging; thrash is a loop that \
            never changed strategy.",
    },
    StatusMarker {
        id: "assumed",
        label_key: "status_markers.assumed",
        icon: "icons/asterisk.svg",
        tone: MarkerTone::Warning,
        threshold: 0.75,
        instructions: "Did the assistant state an assumption about what the user \
            wanted and proceed on it without confirming — wording like 'assuming you \
            meant' or 'I'll go with'? Judge against the `prompt`: flag only an \
            assumption that could plausibly be wrong, not a reasonable default.",
    },
];

/// Prompt text sent to the evaluator is capped so a pasted log cannot crowd
/// out the rest of the state — the ask opens the message, so keep the head.
const PROMPT_STATE_CHARS: usize = 4_000;
/// Earlier user prompts still define scope in a multi-turn session; without
/// them legitimate follow-up work reads as drift. Most recent last.
const PRIOR_PROMPT_STATE_MAX: usize = 3;
const PRIOR_PROMPT_STATE_CHARS: usize = 500;
/// Response text sent to the evaluator is capped so a long final message
/// cannot blow up the request — the marker judgments all read the tail.
const RESPONSE_STATE_CHARS: usize = 10_000;
/// Ordered tool/work activities summarized for the evaluator, most recent
/// last — the sequence is what verification, retry-loop, and scope
/// judgments read, so successful calls matter as much as failed ones.
/// Reads, searches, and lists don't earn lines; they collapse to
/// `explorationCalls`.
const TOOL_SEQUENCE_STATE_MAX: usize = 40;
/// One sequence step's subject is capped so a long command cannot dominate
/// the line.
const TOOL_SEQUENCE_TARGET_CHARS: usize = 80;
/// Failed tool calls summarized for the evaluator, most recent last, each
/// carrying a short tail excerpt of its output.
const TOOL_ERROR_STATE_MAX: usize = 8;
const TOOL_ERROR_EXCERPT_CHARS: usize = 200;
/// Changed paths listed for the evaluator with per-file diff stats; beyond
/// this the list truncates and `filesChangedTotal` keeps the true count.
const FILES_CHANGED_STATE_MAX: usize = 50;

/// The decision-log feature tag these evaluations record under, so the
/// calibration dataset keeps them distinct from ad-hoc `evaluate` calls.
const EVAL_FEATURE: &str = "turn-status";

/// The question map sent with every turn evaluation: one `Choice` for the
/// ending, another for the kind of input needed, plus one `Noul` per flag.
fn status_marker_questions() -> BTreeMap<String, EvalQuestion> {
    let mut questions = BTreeMap::from([(
        ENDING_QUESTION.to_owned(),
        EvalQuestion::Choice {
            instructions: "Which best describes how this turn ended? Judge the closing \
                `response` against the `prompt` and any `priorPrompts` still in play; \
                `toolSequence`, `toolErrors`, and `filesChanged` carry what the turn \
                actually did."
                .to_owned(),
            criteria: ENDING_MARKERS
                .iter()
                .map(|marker| (marker.id.to_owned(), Some(marker.instructions.to_owned())))
                .chain([(ENDING_OTHER_OPTION.to_owned(), None)])
                .collect(),
        },
    )]);
    questions.insert(
        INPUT_QUESTION.to_owned(),
        EvalQuestion::Choice {
            instructions: "If this turn ends awaiting input, what kind of response \
                does the assistant need from the user? Judge the closing `response` \
                against the unfinished work. Otherwise choose `other`."
                .to_owned(),
            criteria: INPUT_MARKERS
                .iter()
                .map(|marker| (marker.id.to_owned(), Some(marker.instructions.to_owned())))
                .chain([(INPUT_OTHER_OPTION.to_owned(), None)])
                .collect(),
        },
    );
    for marker in FLAG_MARKERS {
        questions.insert(
            marker.id.to_owned(),
            EvalQuestion::Noul {
                instructions: marker.instructions.to_owned(),
                criteria: None,
            },
        );
    }
    questions
}

/// The last `max` chars of `text` — conclusions and error lines live at the
/// tail, so that is the end excerpts keep.
fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(max)).collect()
}

/// The first `max` chars of `text` — asks open a message, so that is the
/// end excerpts keep.
fn head_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Pure-exploration kinds — the reads, searches, and directory lists a
/// turn spends orienting. They dwarf the calls markers actually judge, so
/// they collapse to `explorationCalls` rather than spending sequence
/// lines; a *failed* one still earns its step as evidence.
fn is_exploration_kind(kind: ActivityKind) -> bool {
    matches!(
        kind,
        ActivityKind::FileRead | ActivityKind::FileSearch | ActivityKind::FileList
    )
}

/// One step in `toolSequence`: the native tool name (or the activity title
/// when the provider gave none), its prepared subject, and a failure flag.
/// Raw `arguments` stay app-side — `display_target` is the designed-for-
/// display subject, and arguments can carry file contents or secrets into
/// a third-party eval request.
fn tool_sequence_label(activity: &ActivityItem) -> String {
    let mut label = activity
        .tool_name
        .as_deref()
        .unwrap_or(activity.title.as_str())
        .to_owned();
    if let Some(target) = activity.display_target.as_deref() {
        if target.chars().count() > TOOL_SEQUENCE_TARGET_CHARS {
            label.push_str(&format!(
                "({}…)",
                head_chars(target, TOOL_SEQUENCE_TARGET_CHARS)
            ));
        } else {
            label.push_str(&format!("({target})"));
        }
    }
    if activity.failed {
        label.push_str(" (failed)");
    }
    label
}

/// Consecutive identical steps fold into `step ×N` — a retry loop should
/// read as one loud line, not forty rows of the same call.
fn compress_tool_sequence(activities: &[&ActivityItem]) -> Vec<String> {
    let mut runs: Vec<(String, u32)> = Vec::new();
    for activity in activities {
        let label = tool_sequence_label(activity);
        if let Some((last, count)) = runs.last_mut()
            && *last == label
        {
            *count += 1;
        } else {
            runs.push((label, 1));
        }
    }
    runs.into_iter()
        .map(|(label, count)| {
            if count > 1 {
                format!("{label} ×{count}")
            } else {
                label
            }
        })
        .collect()
}

/// The `state` the evaluation judges: the prompt that opened the turn plus
/// the earlier prompts that still define scope, the assistant's closing
/// text, the ordered tool sequence, failures with output tails, per-file
/// diff stats, context occupancy, and the provider's own turn summary.
/// Each marker's instructions read whichever fields its judgment needs.
/// Per-field caps keep the whole state inside ~30k chars — far below what
/// Jev's input pricing makes worth economizing, so the trims only guard
/// latency and judgment focus.
pub(super) fn turn_eval_state(
    session: &AgentSession,
    turn_id: Uuid,
    summary: Option<&str>,
) -> Value {
    let prompt_index = session
        .messages
        .iter()
        .position(|message| message.turn_id == Some(turn_id) && message.role == MessageRole::User);
    let prompt = prompt_index.map(|index| {
        head_chars(
            &session.messages[index..]
                .iter()
                .filter(|message| {
                    message.turn_id == Some(turn_id) && message.role == MessageRole::User
                })
                .map(|message| message.visible_content())
                .collect::<Vec<_>>()
                .join("\n\n"),
            PROMPT_STATE_CHARS,
        )
    });
    let prior_users: Vec<&Message> = session.messages[..prompt_index.unwrap_or(0)]
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .collect();
    let prior_prompts: Vec<String> = prior_users
        .iter()
        .rev()
        .take(PRIOR_PROMPT_STATE_MAX)
        .rev()
        .map(|message| head_chars(message.visible_content(), PRIOR_PROMPT_STATE_CHARS))
        .collect();
    let response = tail_chars(
        &session
            .messages
            .iter()
            .filter(|message| {
                message.turn_id == Some(turn_id) && message.role == MessageRole::Assistant
            })
            .map(|message| message.visible_content())
            .collect::<Vec<_>>()
            .join("\n\n"),
        RESPONSE_STATE_CHARS,
    );
    let activities: Vec<&ActivityItem> = session
        .transcript_blocks
        .iter()
        .filter(|block| block.turn_id == Some(turn_id))
        .flat_map(|block| block.activities.iter())
        .collect();
    let work_activities: Vec<&ActivityItem> = activities
        .iter()
        .filter(|activity| activity.kind != ActivityKind::Reasoning)
        .copied()
        .collect();
    let sequence_activities: Vec<&ActivityItem> = work_activities
        .iter()
        .filter(|activity| {
            activity.kind != ActivityKind::ProjectMap
                && (activity.failed || !is_exploration_kind(activity.kind))
        })
        .copied()
        .collect();
    let exploration_calls = work_activities
        .iter()
        .filter(|activity| !activity.failed && is_exploration_kind(activity.kind))
        .count();
    let tool_sequence = compress_tool_sequence(
        &sequence_activities[sequence_activities
            .len()
            .saturating_sub(TOOL_SEQUENCE_STATE_MAX)..],
    );
    let failed_activities: Vec<&ActivityItem> = activities
        .iter()
        .filter(|activity| activity.failed)
        .copied()
        .collect();
    let tool_errors: Vec<Value> = failed_activities
        [failed_activities.len().saturating_sub(TOOL_ERROR_STATE_MAX)..]
        .iter()
        .map(|activity| {
            json!({
                "tool": activity.tool_name,
                "title": activity.title,
                "outputTail": activity
                    .output
                    .as_deref()
                    .map(|output| tail_chars(output, TOOL_ERROR_EXCERPT_CHARS)),
            })
        })
        .collect();
    let turn = session.turns.iter().find(|turn| turn.id == turn_id);
    let files = turn
        .and_then(|turn| turn.checkpoint.as_ref())
        .map(|checkpoint| checkpoint.files.as_slice())
        .unwrap_or(&[]);
    let files_changed: Vec<String> = files
        .iter()
        .take(FILES_CHANGED_STATE_MAX)
        .map(|file| format!("{} +{}/-{}", file.path, file.additions, file.deletions))
        .collect();
    json!({
        "prompt": prompt,
        "priorPrompts": prior_prompts,
        "response": response,
        "finish": { "success": true, "summary": summary },
        "provider": session.provider.display_name(),
        "model": session.model,
        "contextUsage": session.context_usage.map(|usage| json!({
            "tokens": usage.tokens,
            "window": usage.window,
        })),
        "toolCalls": work_activities.len(),
        "explorationCalls": exploration_calls,
        "toolSequence": tool_sequence,
        "toolErrors": tool_errors,
        "filesChanged": files_changed,
        "filesChangedTotal": files.len(),
    })
}

/// The markers an evaluation cleared, in catalog order — what the footer row
/// renders. The ending `Choice` contributes its winner when the winner's
/// probability clears that marker's bar; each flag `Noul` clears its own
/// threshold. `complete` then yields whenever another marker cleared: it is
/// the nothing-to-see-here chip, and "done" beside a warning or a question
/// reads as a contradiction.
fn cleared_markers(evaluation: &Evaluation) -> Vec<(&'static StatusMarker, f64)> {
    let mut cleared: Vec<(&'static StatusMarker, f64)> = Vec::new();
    if let Some(EvalAnswer::Choice {
        choice,
        probabilities,
        ..
    }) = evaluation.answers.get(ENDING_QUESTION)
        && let Some(marker) = ENDING_MARKERS.iter().find(|marker| marker.id == choice)
    {
        let winner = probabilities.get(choice).copied().unwrap_or(0.0);
        if winner >= marker.threshold {
            let subtype = (marker.id == "awaiting-input")
                .then(|| evaluation.answers.get(INPUT_QUESTION))
                .flatten()
                .and_then(|answer| match answer {
                    EvalAnswer::Choice {
                        choice,
                        probabilities,
                        ..
                    } => INPUT_MARKERS
                        .iter()
                        .find(|subtype| subtype.id == choice)
                        .and_then(|subtype| {
                            let score = probabilities.get(choice).copied().unwrap_or(0.0);
                            (score >= subtype.threshold).then_some((subtype, score))
                        }),
                    _ => None,
                });
            cleared.push(
                subtype
                    .map(|(subtype, score)| (subtype, winner * score))
                    .unwrap_or((marker, winner)),
            );
        }
    }
    for marker in FLAG_MARKERS {
        if let Some(EvalAnswer::Noul { noul }) = evaluation.answers.get(marker.id)
            && *noul >= marker.threshold
        {
            cleared.push((marker, *noul));
        }
    }
    if cleared.len() > 1 {
        cleared.retain(|(marker, _)| marker.id != "complete");
    }
    cleared
}

/// One footer chip: the marker's colored icon and name plus the dim
/// confidence the model reported.
#[track_caller]
fn status_marker_chip(marker: &StatusMarker, probability: f64, theme: &Theme) -> Div {
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
                .child(format!("{}%", (probability * 100.0).round() as u32)),
        )
}

impl Waku {
    /// Status-driven actions take the composer suggestion slot when the
    /// latest settled turn clearly asks for a response from the user.
    pub(super) fn render_status_suggestion(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.state.status_markers_enabled {
            return None;
        }
        let session = self.composer_session()?;
        let turn = session.turns.last()?;
        if turn.status != TurnStatus::Completed {
            return None;
        }
        let turn_id = turn.id;
        let actions = self.turn_status_suggestions.get(&turn_id)?;
        if actions.is_empty() {
            return None;
        }
        let theme = Theme::current(cx);
        Some(
            div().w_full().h(px(0.0)).relative().child(
                div()
                    .absolute()
                    .bottom(px(8.0))
                    .left_0()
                    .right_0()
                    .px(px(20.0 - COMPOSER_OVERHANG))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(CONTENT_MAX_WIDTH + COMPOSER_OVERHANG * 2.0))
                            .mx_auto()
                            .flex()
                            .gap(px(6.0))
                            .children(actions.iter().enumerate().map(|(index, action)| {
                                let (icon_path, label) = match action {
                                    StatusSuggestedAction::Proceed => {
                                        ("icons/chat.svg", tr!("suggestions.proceed"))
                                    }
                                    StatusSuggestedAction::Choose { option } => (
                                        "icons/chat.svg",
                                        tr!("suggestions.choose_option", option = option),
                                    ),
                                    StatusSuggestedAction::AddDetails => {
                                        ("icons/chat.svg", tr!("suggestions.add_details"))
                                    }
                                    StatusSuggestedAction::ChooseManually => {
                                        ("icons/chat.svg", tr!("suggestions.choose_manually"))
                                    }
                                };
                                let action = action.clone();
                                let keyboard_action = action.clone();
                                div()
                                    .id(format!("status-suggestion-{turn_id}-{index}"))
                                    .flex()
                                    .items_center()
                                    .gap(px(5.0))
                                    .h(px(24.0))
                                    .px(px(9.0))
                                    .rounded(px(8.0))
                                    .border(hairline())
                                    .border_color(theme.border_subtle)
                                    .bg(theme.raised)
                                    .cursor_default()
                                    .track_focus(&self.status_suggestion_focuses[index])
                                    .tab_index(0)
                                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                                    .hover(|element| element.bg(theme.overlay_strong))
                                    .child(icon(icon_path, 11.0, theme.text_secondary))
                                    .child(label)
                                    .tooltip(Tooltip::text(tr!("suggestions.tooltip")))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.accept_status_suggestion(turn_id, &action, window, cx);
                                    }))
                                    .on_key_down(cx.listener(
                                        move |this, event: &KeyDownEvent, window, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                this.accept_status_suggestion(
                                                    turn_id,
                                                    &keyboard_action,
                                                    window,
                                                    cx,
                                                );
                                                cx.stop_propagation();
                                            }
                                        },
                                    ))
                            })),
                    ),
            ),
        )
    }

    fn accept_status_suggestion(
        &mut self,
        turn_id: Uuid,
        action: &StatusSuggestedAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.composer_session() else {
            return;
        };
        if !self.state.status_markers_enabled
            || session
                .turns
                .last()
                .is_none_or(|turn| turn.id != turn_id || turn.status != TurnStatus::Completed)
            || !self
                .turn_status_suggestions
                .get(&turn_id)
                .is_some_and(|actions| actions.contains(action))
        {
            return;
        }
        let session_id = session.id;
        match action {
            StatusSuggestedAction::Proceed => {
                self.turn_status_suggestions.insert(turn_id, Vec::new());
                self.submit_composer_submission_to(
                    session_id,
                    ComposerSubmission::plain(tr!("suggestions.proceed")),
                    cx,
                );
            }
            StatusSuggestedAction::Choose { option } => {
                let prompt = tr!("suggestions.chosen_option", option = option);
                self.turn_status_suggestions.insert(turn_id, Vec::new());
                self.submit_composer_submission_to(
                    session_id,
                    ComposerSubmission::plain(prompt),
                    cx,
                );
            }
            StatusSuggestedAction::AddDetails | StatusSuggestedAction::ChooseManually => {
                window.focus(&self.composer_focus(cx), cx);
            }
        }
    }

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
    /// scores, off the UI thread. An unusable eval backend on that daemon —
    /// unconfigured or missing its credential — means the feature simply
    /// does not fire: no row, no error, no doomed call.
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
        if daemon
            .settings()
            .eval
            .is_none_or(|eval| eval.credential_missing())
        {
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
                    if let Some(session) = self
                        .state
                        .sessions
                        .iter()
                        .find(|session| session.turns.iter().any(|turn| turn.id == turn_id))
                    {
                        let response = session
                            .messages
                            .iter()
                            .filter(|message| {
                                message.turn_id == Some(turn_id)
                                    && message.role == MessageRole::Assistant
                            })
                            .map(|message| message.visible_content())
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        let actions = suggested_actions(&evaluation, &response);
                        if !actions.is_empty() {
                            self.turn_status_suggestions.insert(turn_id, actions);
                        }
                    }
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
                    cleared.into_iter().map(|(marker, probability)| {
                        status_marker_chip(marker, probability, theme)
                    }),
                )
                .into_any_element(),
        )
    }

    /// The settled last turn's chips floated over the transcript's bottom
    /// edge while its footer row sits below the fold, so a reader partway up
    /// a long response still sees how the turn resolved. The footer keeps
    /// the real row; activating the float scrolls the footer back into
    /// view. Only the session's last turn qualifies — pinning an older
    /// turn's verdict under a newer response would misattribute it.
    pub(super) fn render_floating_status_markers(
        &self,
        transcript_rows: &ListState,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let turn_id = self.selected_session()?.turns.last()?.id;
        let cleared = cleared_markers(self.turn_status_markers.get(&turn_id)?);
        if cleared.is_empty() {
            return None;
        }
        let footer_row = self
            .transcript_row_kinds
            .borrow()
            .iter()
            .rposition(|kind| {
                matches!(kind, TranscriptRowKind::ResponseFooter(id, _) if *id == turn_id)
            })?;
        // Unmeasured rows report `None`: no float until the footer has been
        // laid out once and the list can say where it sits.
        if transcript_rows.item_is_below_viewport(footer_row) != Some(true) {
            return None;
        }
        let focus = self.transcript_control_focus("transcript-status-markers", cx);
        Some(
            div()
                .absolute()
                .bottom(px(8.0))
                .left_0()
                .right_0()
                // Same insets and content width as transcript rows, so the
                // float's left edge lands on the column the footer lives in.
                .px(px(20.0))
                .child(
                    div()
                        .w_full()
                        .max_w(px(CONTENT_MAX_WIDTH))
                        .mx_auto()
                        .flex()
                        .child(
                            div()
                                .id("transcript-status-markers")
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .py(px(5.0))
                                .px(px(9.0))
                                .rounded(px(8.0))
                                .border(hairline())
                                .border_color(theme.border_subtle)
                                .bg(theme.raised)
                                .shadow_xs()
                                .cursor_default()
                                .track_focus(&focus)
                                .tab_index(0)
                                .focus_visible(|style| style.bg(theme.focus_highlight()))
                                .children(cleared.into_iter().map(|(marker, probability)| {
                                    status_marker_chip(marker, probability, theme)
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.active_transcript_rows()
                                        .scroll_to_reveal_item(footer_row);
                                    cx.notify();
                                }))
                                .on_key_down(cx.listener(
                                    move |this, event: &KeyDownEvent, _, cx| {
                                        if matches!(
                                            event.keystroke.key.as_str(),
                                            "enter" | "space"
                                        ) {
                                            this.active_transcript_rows()
                                                .scroll_to_reveal_item(footer_row);
                                            cx.stop_propagation();
                                        }
                                    },
                                )),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Forget every marker verdict and pending request; the feature's state
    /// is runtime-only by design, so toggling off wipes it rather than
    /// leaving stale chips on screen.
    pub(super) fn clear_status_markers(&mut self) {
        self.turn_status_markers.clear();
        self.turn_status_suggestions.clear();
        self.pending_status_marker_turns.clear();
        self.status_marker_in_flight.clear();
    }
}

#[cfg(test)]
mod tests {
    use waku_protocol::model::{
        Checkpoint, CheckpointFile, CheckpointStatus, TranscriptBlock, TurnStatus,
    };

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

    fn ending_choice(choice: &str, probabilities: &[(&str, f64)]) -> EvalAnswer {
        EvalAnswer::Choice {
            choice: choice.to_owned(),
            confidence: None,
            probabilities: probabilities
                .iter()
                .map(|(id, probability)| (id.to_string(), *probability))
                .collect(),
        }
    }

    #[test]
    fn questions_split_the_ending_choice_from_flag_nouls() {
        let questions = status_marker_questions();
        assert_eq!(questions.len(), FLAG_MARKERS.len() + 2);
        let Some(EvalQuestion::Choice { criteria, .. }) = questions.get(ENDING_QUESTION) else {
            panic!("the ending question must be a choice");
        };
        for marker in ENDING_MARKERS {
            assert!(criteria.contains_key(marker.id));
        }
        assert!(criteria.contains_key(ENDING_OTHER_OPTION));
        let Some(EvalQuestion::Choice { criteria, .. }) = questions.get(INPUT_QUESTION) else {
            panic!("the input subtype question must be a choice");
        };
        for marker in INPUT_MARKERS {
            assert!(criteria.contains_key(marker.id));
        }
        assert!(criteria.contains_key(INPUT_OTHER_OPTION));
        for marker in FLAG_MARKERS {
            assert!(matches!(
                questions.get(marker.id),
                Some(EvalQuestion::Noul { .. })
            ));
        }
    }

    #[test]
    fn awaiting_input_uses_a_confident_subtype_or_the_generic_label() {
        let answers = |input_choice: &str, confidence: f64| {
            evaluation(BTreeMap::from([
                (
                    ENDING_QUESTION.to_owned(),
                    ending_choice(
                        "awaiting-input",
                        &[("awaiting-input", 0.88), ("other", 0.12)],
                    ),
                ),
                (
                    INPUT_QUESTION.to_owned(),
                    ending_choice(input_choice, &[(input_choice, confidence)]),
                ),
            ]))
        };
        for (choice, expected) in [
            ("go-ahead", "go-ahead"),
            ("decision", "decision"),
            ("details", "details"),
        ] {
            let markers = cleared_markers(&answers(choice, 0.82));
            assert_eq!(markers[0].0.id, expected);
            assert!((markers[0].1 - 0.88 * 0.82).abs() < f64::EPSILON);
        }
        assert_eq!(
            cleared_markers(&answers("go-ahead", 0.40))[0].0.id,
            "awaiting-input"
        );
        assert_eq!(
            cleared_markers(&answers("other", 0.90))[0].0.id,
            "awaiting-input"
        );
    }

    #[test]
    fn status_suggestions_match_the_input_needed() {
        let verdict = |kind: &str| {
            evaluation(BTreeMap::from([
                (
                    ENDING_QUESTION.to_owned(),
                    ending_choice("awaiting-input", &[("awaiting-input", 0.90)]),
                ),
                (
                    INPUT_QUESTION.to_owned(),
                    ending_choice(kind, &[(kind, 0.90)]),
                ),
            ]))
        };
        assert_eq!(
            suggested_actions(&verdict("go-ahead"), "Want me to proceed?"),
            [StatusSuggestedAction::Proceed]
        );
        assert_eq!(
            suggested_actions(&verdict("details"), "Which account?"),
            [StatusSuggestedAction::AddDetails]
        );
        assert_eq!(
            suggested_actions(
                &verdict("decision"),
                "1. **SQLite** — local\n2. **JSON** — simple\nWhich do you prefer?"
            ),
            [
                StatusSuggestedAction::Choose {
                    option: "1. SQLite".to_owned()
                },
                StatusSuggestedAction::Choose {
                    option: "2. JSON".to_owned()
                },
            ]
        );
        assert_eq!(
            suggested_actions(&verdict("decision"), "Should we use SQLite or JSON?"),
            [StatusSuggestedAction::ChooseManually]
        );
        assert!(suggested_actions(&verdict("other"), "Want me to proceed?").is_empty());
    }

    #[test]
    fn cleared_markers_pick_the_ending_winner_and_clear_flags() {
        let evaluation = evaluation(
            [
                (
                    ENDING_QUESTION.to_owned(),
                    ending_choice(
                        "blocked",
                        &[
                            ("complete", 0.05),
                            ("awaiting-input", 0.10),
                            ("partial", 0.15),
                            ("blocked", 0.60),
                            ("failed", 0.05),
                            ("other", 0.05),
                        ],
                    ),
                ),
                ("unverified".to_owned(), EvalAnswer::Noul { noul: 0.80 }),
                ("drifted".to_owned(), EvalAnswer::Noul { noul: 0.50 }),
            ]
            .into_iter()
            .collect(),
        );
        let ids: Vec<&str> = cleared_markers(&evaluation)
            .iter()
            .map(|(marker, _)| marker.id)
            .collect();
        // The ending winner clears blocked's 0.45 bar; unverified's 0.80
        // clears 0.75 while drifted's 0.50 misses 0.70.
        assert_eq!(ids, ["blocked", "unverified"]);
    }

    #[test]
    fn a_weak_ending_winner_and_the_other_bucket_render_nothing() {
        let split = evaluation(
            [(
                ENDING_QUESTION.to_owned(),
                ending_choice(
                    "complete",
                    &[("complete", 0.40), ("partial", 0.35), ("other", 0.25)],
                ),
            )]
            .into_iter()
            .collect(),
        );
        assert!(cleared_markers(&split).is_empty());

        let other = evaluation(
            [(
                ENDING_QUESTION.to_owned(),
                ending_choice("other", &[("other", 0.90), ("complete", 0.10)]),
            )]
            .into_iter()
            .collect(),
        );
        assert!(cleared_markers(&other).is_empty());
    }

    #[test]
    fn complete_yields_the_footer_to_any_flag() {
        let done_unchecked = evaluation(
            [
                (
                    ENDING_QUESTION.to_owned(),
                    ending_choice("complete", &[("complete", 0.90), ("other", 0.10)]),
                ),
                ("unverified".to_owned(), EvalAnswer::Noul { noul: 0.90 }),
            ]
            .into_iter()
            .collect(),
        );
        let ids: Vec<&str> = cleared_markers(&done_unchecked)
            .iter()
            .map(|(marker, _)| marker.id)
            .collect();
        assert_eq!(ids, ["unverified"]);

        let clean = evaluation(
            [(
                ENDING_QUESTION.to_owned(),
                ending_choice("complete", &[("complete", 0.90), ("other", 0.10)]),
            )]
            .into_iter()
            .collect(),
        );
        let ids: Vec<&str> = cleared_markers(&clean)
            .iter()
            .map(|(marker, _)| marker.id)
            .collect();
        assert_eq!(ids, ["complete"]);
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

    #[test]
    fn turn_state_carries_scope_sequence_and_diff_stats() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("Fix the bug");
        session.push_message(MessageRole::Assistant, "Fixed it.");
        session.finish_active_turn(TurnStatus::Completed);
        let turn_id = session.begin_turn("Also update the docs");

        let mut edit = ActivityItem::new(None, ActivityKind::FileChange, "Edit", None, true)
            .with_tool_name(Some("edit_file"));
        edit.display_target = Some("src/a.rs".to_owned());
        let mut test = ActivityItem::new(None, ActivityKind::Command, "test", None, true)
            .with_tool_name(Some("bash"));
        test.display_target = Some("cargo test".to_owned());
        let mut build = ActivityItem::new(None, ActivityKind::Command, "build", None, true)
            .with_tool_name(Some("bash"));
        build.display_target = Some("cargo build".to_owned());
        build.failed = true;
        build.output = Some("error[E0308]: mismatched types".to_owned());
        let mut read = ActivityItem::new(None, ActivityKind::FileRead, "read", None, true)
            .with_tool_name(Some("read_file"));
        read.display_target = Some("src/ok.rs".to_owned());
        let mut denied = ActivityItem::new(None, ActivityKind::FileRead, "read", None, true)
            .with_tool_name(Some("read_file"));
        denied.display_target = Some("/etc/shadow".to_owned());
        denied.failed = true;
        denied.output = Some("permission denied".to_owned());
        let search = ActivityItem::new(None, ActivityKind::FileSearch, "grep", None, true)
            .with_tool_name(Some("grep"));
        session.transcript_blocks.push(TranscriptBlock {
            after_message: 2,
            turn_id: Some(turn_id),
            activities: vec![
                read,
                search,
                edit,
                test.clone(),
                test,
                build,
                denied,
                ActivityItem::from_reasoning(
                    ReasoningBlock {
                        content: "done".into(),
                        started_at_ms: 0,
                        finished_at_ms: 1,
                    },
                    true,
                ),
            ],
        });
        session.turns.last_mut().unwrap().checkpoint = Some(Checkpoint {
            turn_count: 2,
            git_ref: "deadbeef".to_owned(),
            status: CheckpointStatus::Ready,
            files: vec![
                CheckpointFile {
                    path: "src/a.rs".to_owned(),
                    additions: 10,
                    deletions: 2,
                },
                CheckpointFile {
                    path: "README.md".to_owned(),
                    additions: 4,
                    deletions: 0,
                },
            ],
            additions: 14,
            deletions: 2,
            created_at: 0,
        });
        session.context_usage = Some(ContextUsage {
            tokens: 12_000,
            window: Some(200_000),
        });
        session.push_message(MessageRole::Assistant, "Docs updated.");

        let state = turn_eval_state(&session, turn_id, None);
        // The earlier turn's ask is scope, not drift.
        assert_eq!(state["prompt"], "Also update the docs");
        assert_eq!(state["priorPrompts"], json!(["Fix the bug"]));
        // Reasoning stays out of the sequence; a repeated call folds to ×N;
        // successful reads/searches collapse to a count while a failed one
        // keeps its step.
        assert_eq!(
            state["toolSequence"],
            json!([
                "edit_file(src/a.rs)",
                "bash(cargo test) ×2",
                "bash(cargo build) (failed)",
                "read_file(/etc/shadow) (failed)"
            ])
        );
        assert_eq!(state["toolCalls"], 7);
        assert_eq!(state["explorationCalls"], 2);
        assert_eq!(
            state["toolErrors"],
            json!([
                {
                    "tool": "bash",
                    "title": "build",
                    "outputTail": "error[E0308]: mismatched types",
                },
                {
                    "tool": "read_file",
                    "title": "read",
                    "outputTail": "permission denied",
                }
            ])
        );
        assert_eq!(
            state["filesChanged"],
            json!(["src/a.rs +10/-2", "README.md +4/-0"])
        );
        assert_eq!(state["filesChangedTotal"], 2);
        assert_eq!(
            state["contextUsage"],
            json!({"tokens": 12_000, "window": 200_000})
        );
    }
}
