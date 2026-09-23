//! Next-action predictions, shadow mode: while the experiment is on, each
//! naturally settled turn gets one evaluation call that classifies the task
//! and picks the user's likely next move from a fixed candidate set. Nothing
//! renders — a prediction sits pending until the user's next journaled
//! action resolves it, and every verdict lands in
//! `~/.goddard/action-predictions.jsonl` so calibration is measured before
//! any suggestion UI ships.
//!
//! The action journal (`~/.goddard/actions.jsonl`) is the raw material: a
//! closed vocabulary of consequential moves — prompt sends, Git operations,
//! session lifecycle, terminal runs — recorded at their dispatch sites.
//! Navigation, mid-turn controls, and consent stay out of it; a journal
//! entry is an action id plus a session, never content, and the file never
//! leaves the machine. The pending window ends at the session's next
//! prompt: intervening actions only resolve the prediction they fulfill.

use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use waku_client::git::PullStrategy;
use waku_protocol::eval::{EvalAnswer, EvalQuestion, Evaluation};
use waku_protocol::model::{AgentSession, SessionWorkspace};

use crate::ui::shortcut::ShortcutHint;

use super::status_markers::StatusSuggestedAction;
use super::*;

/// One consequential user action, recorded at its dispatch site. The
/// journal file speaks `id()` — the same vocabulary the `nextAction`
/// question's options use — so a prediction and the action that resolves
/// it compare as strings.
#[derive(Clone, Copy, Debug)]
pub(super) enum JournalAction {
    /// A composer submission went out — bespoke text or, when `canned`
    /// names one of the fixed follow-ups, the prompt a suggestion chip
    /// would send. A user typing "run the tests" by hand is the same
    /// outcome as the chip sending it.
    PromptSend {
        canned: Option<&'static str>,
    },
    GitCommit,
    GitPush,
    /// The Git panel's Sync — pull upstream into the checked-out branch.
    GitSync,
    /// The sync strip's fetch/push and the landed-notice's PushBase or
    /// SyncBase: moves the base branch, not the checkout.
    GitSyncBase,
    GitLand,
    GitRebase,
    /// A message rewind's workspace restore — "revert to here".
    GitRevert,
    /// A shell command finished in a user terminal, or an app-launched
    /// custom command ran.
    TerminalRun,
    SessionArchive,
    SessionUnarchive,
    SessionNew,
    WorktreeNew,
    ModelSwitch,
}

impl JournalAction {
    /// The candidate id this entry fulfills.
    fn id(&self) -> &'static str {
        match self {
            Self::PromptSend { canned } => canned.unwrap_or("new-prompt"),
            Self::GitCommit => "commit",
            Self::GitPush => "push",
            Self::GitSync => "sync",
            Self::GitSyncBase => "sync-base",
            Self::GitLand => "land",
            Self::GitRebase => "rebase",
            Self::GitRevert => "revert",
            Self::TerminalRun => "terminal-command",
            Self::SessionArchive => "archive",
            Self::SessionUnarchive => "unarchive",
            Self::SessionNew => "new-task",
            Self::WorktreeNew => "new-worktree",
            Self::ModelSwitch => "switch-model",
        }
    }

    /// Prompt sends close the pending window whether or not they fulfill a
    /// prediction: "before the next prompt" ends here.
    fn ends_prediction_window(&self) -> bool {
        matches!(self, Self::PromptSend { .. })
    }
}

/// Stable ids and localized defaults for suggested prompts. The settings
/// page edits the payload while predictions keep these ids unchanged.
pub(super) const CANNED_PROMPTS: &[(&str, &str)] = &[
    ("proceed", "suggestions.proceed"),
    ("keep-going", "suggestions.keep_going"),
    ("run-tests", "suggestions.run_tests"),
    ("fix-errors", "suggestions.fix_errors"),
    ("commit-changes", "suggestions.commit_changes"),
    ("add-tests", "suggestions.add_tests"),
    ("review-changes", "suggestions.review_changes"),
    ("open-pr", "suggestions.open_pr"),
];
pub(super) const CHOICE_PROMPT_ID: &str = "choose-option";

pub(super) fn default_suggested_prompt(action: &str) -> Option<String> {
    if action == CHOICE_PROMPT_ID {
        return Some(tr!("suggestions.chosen_option", option = "{option}"));
    }
    let (_, key) = CANNED_PROMPTS.iter().find(|(id, _)| *id == action)?;
    Some(tr!(key))
}

pub(super) fn suggested_prompt(
    action: &str,
    overrides: &BTreeMap<String, String>,
    option: Option<&str>,
) -> Option<String> {
    let default = default_suggested_prompt(action)?;
    let template = overrides
        .get(action)
        .filter(|value| valid_suggested_prompt(action, value))
        .cloned()
        .unwrap_or(default);
    if action == CHOICE_PROMPT_ID {
        Some(template.replace("{option}", option?))
    } else {
        Some(template)
    }
}

pub(super) fn valid_suggested_prompt(action: &str, prompt: &str) -> bool {
    let trimmed = prompt.trim();
    !trimmed.is_empty()
        && trimmed.chars().count() <= 2_000
        && (action != CHOICE_PROMPT_ID || trimmed.matches("{option}").count() == 1)
}

/// The canned id for a submitted prompt, when its text is one of the fixed
/// follow-ups — case- and whitespace-insensitive, so a hand-typed
/// equivalent counts as the same outcome.
pub(super) fn canned_prompt_id(
    prompt: &str,
    overrides: &BTreeMap<String, String>,
) -> Option<&'static str> {
    let normalized = prompt.trim();
    CANNED_PROMPTS
        .iter()
        .find(|(id, _)| {
            suggested_prompt(id, overrides, None)
                .is_some_and(|configured| configured.trim().eq_ignore_ascii_case(normalized))
        })
        .map(|(id, _)| *id)
}

/// Candidate ids a chip can execute itself. Everything else in the
/// vocabulary resolves predictions but never renders — `new-prompt` and
/// `other` are outcomes, session lifecycle picks are better left to the
/// sidebar, and `revert`/`terminal-command` stay manual in v1.
const ACTIONABLE_SUGGESTIONS: &[&str] = &[
    "keep-going",
    "run-tests",
    "fix-errors",
    "commit-changes",
    "add-tests",
    "review-changes",
    "open-pr",
    "commit",
    "push",
    "sync",
    "land",
];

/// A suggestion renders only when the model is confident and committed:
/// the argmax clears this probability and beats the runner-up by this
/// margin. Both are starting values to revisit against the shadow log.
const SUGGESTION_MIN_PROBABILITY: f64 = 0.5;
const SUGGESTION_MIN_MARGIN: f64 = 0.15;

/// One journaled action held in memory — the tail `recentActions` reads.
/// The file carries the same shape: `{"at", "session", "action"}`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct JournalRecord {
    pub at: u64,
    pub session: Option<Uuid>,
    pub action: String,
}

/// A scored `nextAction` waiting on its outcome.
pub(super) struct PendingActionPrediction {
    pub id: Uuid,
    pub session_id: Uuid,
    /// The option the model ranked first.
    pub predicted: String,
    /// Every option the question offered — an unlisted action hitting an
    /// `other` prediction is a hit, not silence.
    pub candidates: Vec<String>,
    /// The user clicked this prediction's chip — adoption, distinct from
    /// the correctness outcome the resolution records.
    pub adopted: bool,
}

/// The floating suggestion above the composer — the pending prediction's
/// gated pick. It clears when the prediction resolves, whatever the
/// outcome: fulfilled, or the window ended with the next prompt.
#[derive(Clone)]
pub(super) struct ActionSuggestion {
    /// The prediction this chip stands in for — adoption marks and clearing
    /// both key off it.
    pub prediction_id: Uuid,
    pub session_id: Uuid,
    /// The candidate id; `suggestion_parts` maps it to label, icon, and
    /// dispatch.
    pub action: &'static str,
}

/// What a chip does when clicked.
enum SuggestionDispatch {
    /// Submit the fixed follow-up through the normal send path — it
    /// journals as `prompt_send` with the canned id, resolving the
    /// prediction itself.
    Prompt(&'static str),
    /// Open the commit dialog — reviewing the generated message is part of
    /// the commit, so the chip opens it rather than committing blind.
    Commit,
    /// Push the session's workspace through the panel's op machinery.
    Push,
    /// Pull upstream into the session's workspace.
    Sync,
    /// Land the composer session's worktree onto its base.
    Land,
}

/// The dispatch behind an actionable candidate id.
fn suggestion_dispatch(action: &str) -> Option<SuggestionDispatch> {
    match action {
        "commit" => Some(SuggestionDispatch::Commit),
        "push" => Some(SuggestionDispatch::Push),
        "sync" => Some(SuggestionDispatch::Sync),
        "land" => Some(SuggestionDispatch::Land),
        canned => CANNED_PROMPTS
            .iter()
            .find(|(id, _)| *id == canned)
            .map(|(id, _)| SuggestionDispatch::Prompt(id)),
    }
}

/// The pick worth rendering: an actionable argmax that clears the
/// confidence gate. Returns the candidate's static id for the chip.
fn gated_suggestion(choice: &str, probabilities: &BTreeMap<String, f64>) -> Option<&'static str> {
    let action = *ACTIONABLE_SUGGESTIONS.iter().find(|id| **id == choice)?;
    let top = probabilities.get(choice).copied().unwrap_or(0.0);
    let runner_up = probabilities
        .iter()
        .filter(|(id, _)| id.as_str() != choice)
        .map(|(_, probability)| *probability)
        .fold(0.0, f64::max);
    (top >= SUGGESTION_MIN_PROBABILITY && top - runner_up >= SUGGESTION_MIN_MARGIN)
        .then_some(action)
}

/// The decision-log feature tag these evaluations record under, so the
/// daemon's log keeps them distinct from the turn-status calls.
const EVAL_FEATURE: &str = "next-action";

/// Journal entries kept in memory for the `recentActions` state field; the
/// file keeps everything, the prior only needs the recent shape.
const JOURNAL_TAIL_MAX: usize = 200;

/// Journal file size at which a load rewrites it down to the in-memory
/// tail — a slow-leak guard, not per-append work.
const JOURNAL_FILE_MAX_BYTES: u64 = 256 * 1024;

fn journal_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".goddard")
        .join("actions.jsonl")
}

fn prediction_log_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".goddard")
        .join("action-predictions.jsonl")
}

/// Append one JSON line; a write failure is silent — the journal is
/// instrumentation, never a reason to disturb the action it recorded.
fn append_jsonl(path: &Path, value: &impl Serialize) {
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    if let Ok(line) = serde_json::to_string(value) {
        let _ = writeln!(file, "{line}");
    }
}

/// Read the journal's tail back on launch so a restart does not reset the
/// behavioral prior. Overgrown files rewrite down to the tail here — the
/// one moment a rewrite costs nothing.
pub(super) fn load_action_journal() -> VecDeque<JournalRecord> {
    let path = journal_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return VecDeque::new();
    };
    let mut records: VecDeque<JournalRecord> = VecDeque::new();
    for line in contents.lines() {
        let Ok(record) = serde_json::from_str::<JournalRecord>(line) else {
            continue;
        };
        records.push_back(record);
    }
    while records.len() > JOURNAL_TAIL_MAX {
        records.pop_front();
    }
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > JOURNAL_FILE_MAX_BYTES) {
        let tail: Vec<String> = records
            .iter()
            .filter_map(|record| serde_json::to_string(record).ok())
            .collect();
        let _ = std::fs::write(&path, tail.join("\n") + "\n");
    }
    records
}

/// The options the `nextAction` question offers: feasibility-gated
/// candidates plus `new-prompt`, which names the common case — the user
/// writing their own message — so `other` stays honest about genuinely
/// unenumerated moves.
fn next_action_candidates(
    session: &AgentSession,
    turn_id: Uuid,
) -> Vec<(&'static str, &'static str)> {
    let turn = session.turns.iter().find(|turn| turn.id == turn_id);
    let files_changed = turn
        .and_then(|turn| turn.checkpoint.as_ref())
        .is_some_and(|checkpoint| !checkpoint.files.is_empty());
    let saw_failure = session
        .transcript_blocks
        .iter()
        .filter(|block| block.turn_id == Some(turn_id))
        .flat_map(|block| block.activities.iter())
        .any(|activity| activity.failed);
    let mut candidates: Vec<(&'static str, &'static str)> = vec![
        ("keep-going", "Tell the agent to keep going"),
        ("archive", "Archive this session"),
        ("new-task", "Start a new task"),
        ("new-worktree", "Start a task in a new worktree"),
        (
            "new-prompt",
            "Write and send a prompt of their own — something not listed",
        ),
    ];
    if files_changed {
        candidates.extend([
            ("run-tests", "Ask the agent to run the tests"),
            ("commit-changes", "Ask the agent to commit the changes"),
            ("add-tests", "Ask the agent to add tests for the change"),
            ("review-changes", "Ask the agent to review the changes"),
            ("open-pr", "Ask the agent to push and open a PR"),
            ("commit", "Commit the changes through the Git UI"),
            ("push", "Push the work to its remote"),
            ("revert", "Rewind the workspace to before this turn"),
        ]);
    }
    if saw_failure {
        candidates.push(("fix-errors", "Ask the agent to fix the errors"));
    }
    if matches!(session.workspace, SessionWorkspace::Worktree { .. }) {
        candidates.push(("land", "Land the worktree's commits onto its base"));
    }
    candidates
}

/// The question map sent with every prediction call: `taskType` rides
/// along purely for the calibration log — it answers later whether some
/// task kinds predict better — while `nextAction` is the shadow pick.
fn action_prediction_questions(
    candidates: Vec<(&'static str, &'static str)>,
) -> BTreeMap<String, EvalQuestion> {
    let mut criteria: BTreeMap<String, Option<String>> = candidates
        .into_iter()
        .map(|(id, description)| (id.to_owned(), Some(description.to_owned())))
        .collect();
    criteria.insert("other".to_owned(), None);
    BTreeMap::from([
        (
            "taskType".to_owned(),
            EvalQuestion::Choice {
                instructions: "Classify what the user asked this turn to do.".to_owned(),
                criteria: [
                    ("fix", Some("Repair a bug, failure, or regression")),
                    ("feature", Some("Build new behavior or UI")),
                    ("refactor", Some("Restructure without changing behavior")),
                    (
                        "question",
                        Some("Answer or explain — no code change expected"),
                    ),
                    ("chore", Some("Maintenance: deps, config, cleanup")),
                    ("other", None),
                ]
                .into_iter()
                .map(|(id, description)| (id.to_owned(), description.map(str::to_owned)))
                .collect(),
            },
        ),
        (
            "nextAction".to_owned(),
            EvalQuestion::Choice {
                instructions: "The assistant's turn just settled. Which of these is the user \
                    most likely to do before their next prompt in this session? Judge from the \
                    turn's outcome and the user's recent actions."
                    .to_owned(),
                criteria,
            },
        ),
    ])
}

impl Waku {
    /// Route a settled turn into prediction: score it now when its session
    /// is on screen, queue it behind the session otherwise — the next open
    /// drains the queue. Same shape as the status-marker routing a line up.
    pub(super) fn note_turn_finished_for_action_predictions(
        &mut self,
        session_id: Uuid,
        turn_id: Option<Uuid>,
        summary: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(turn_id) = turn_id else {
            return;
        };
        if !self.state.action_predictions_enabled {
            return;
        }
        if self.state.selected_session == Some(session_id) {
            self.request_action_prediction_eval(session_id, turn_id, summary, cx);
        } else {
            self.pending_action_prediction_turns
                .entry(session_id)
                .or_default()
                .push((turn_id, summary));
        }
    }

    /// Evaluate every turn that settled while `session_id` was off screen.
    /// Called when the session becomes the selected one.
    pub(super) fn drain_pending_action_prediction_turns(
        &mut self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_action_prediction_turns.remove(&session_id) else {
            return;
        };
        for (turn_id, summary) in pending {
            self.request_action_prediction_eval(session_id, turn_id, summary, cx);
        }
    }

    /// Build the turn's state — the status-marker payload plus the fields
    /// a next-action judgment reads — and ask its session's daemon for the
    /// prediction, off the UI thread. An unusable eval backend —
    /// unconfigured or missing its credential — means the feature simply
    /// does not fire.
    fn request_action_prediction_eval(
        &mut self,
        session_id: Uuid,
        turn_id: Uuid,
        summary: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.state.action_predictions_enabled
            || self.action_prediction_in_flight.contains(&turn_id)
        {
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
        let mut state = status_markers::turn_eval_state(session, turn_id, summary.as_deref());
        if let Some(object) = state.as_object_mut() {
            object.insert("contextUsage".to_owned(), json!(session.context_usage));
            object.insert(
                "worktree".to_owned(),
                json!(session.workspace.is_worktree()),
            );
            object.insert(
                "recentActions".to_owned(),
                json!(
                    self.action_journal
                        .iter()
                        .map(|record| record.action.as_str())
                        .collect::<Vec<_>>()
                ),
            );
        }
        let questions = action_prediction_questions(next_action_candidates(session, turn_id));
        let tx = self.action_prediction_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        self.action_prediction_in_flight.insert(turn_id);
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
                if tx.send((session_id, turn_id, result)).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .detach();
    }

    /// Land answered predictions. A failed call drops quietly — the pending
    /// list simply never gains the entry, matching every other eval
    /// fallback.
    pub(super) fn drain_action_prediction_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok((session_id, turn_id, result)) = self.action_prediction_events.try_recv() {
            self.action_prediction_in_flight.remove(&turn_id);
            match result {
                Ok(evaluation) => {
                    self.log_prediction(session_id, turn_id, &evaluation);
                    changed = true;
                }
                Err(error) => {
                    eprintln!("Goddard: action prediction evaluation failed: {error}");
                }
            }
        }
        changed
    }

    /// Record the model's pick as a pending prediction and append the
    /// `prediction` line — candidates and the full distribution included,
    /// so the log can answer both "was the argmax right" and "was the
    /// distribution honest" at analysis time. `taskType` is logged for the
    /// same reason and never read at runtime.
    fn log_prediction(&mut self, session_id: Uuid, turn_id: Uuid, evaluation: &Evaluation) {
        // Incognito sessions write nothing to the prediction log either.
        if self.session_incognito(session_id) {
            return;
        }
        let Some(EvalAnswer::Choice {
            choice,
            probabilities,
            ..
        }) = evaluation.answers.get("nextAction")
        else {
            return;
        };
        let task_type = match evaluation.answers.get("taskType") {
            Some(EvalAnswer::Choice { choice, .. }) => Some(choice.clone()),
            _ => None,
        };
        let prediction = PendingActionPrediction {
            id: Uuid::new_v4(),
            session_id,
            predicted: choice.clone(),
            candidates: probabilities.keys().cloned().collect(),
            adopted: false,
        };
        // The newest gated pick owns the chip; an unconfident answer leaves
        // whatever the previous prediction suggested — the user never sees
        // a suggestion swap for a weak signal.
        if let Some(action) = gated_suggestion(choice, probabilities) {
            self.action_suggestion = Some(ActionSuggestion {
                prediction_id: prediction.id,
                session_id,
                action,
            });
        }
        append_jsonl(
            &prediction_log_path(),
            &json!({
                "type": "prediction",
                "id": prediction.id,
                "at": unix_time(),
                "session": session_id,
                "turn": turn_id,
                "predicted": prediction.predicted,
                "taskType": task_type,
                "probabilities": probabilities,
                "candidates": prediction.candidates,
            }),
        );
        self.pending_action_predictions.push(prediction);
    }

    /// The journal's one entry point: every consequential-action dispatch
    /// site calls through here. Appends the record, keeps the in-memory
    /// tail bounded, and resolves any predictions this action settles.
    /// Nothing is journaled while the experiment is off.
    pub(super) fn record_action(&mut self, session: Option<Uuid>, action: JournalAction) {
        if !self.state.action_predictions_enabled {
            return;
        }
        // Incognito sessions leave no trace — a journaled row would persist
        // the session id past the session's own lifetime.
        if session.is_some_and(|id| self.session_incognito(id)) {
            return;
        }
        let at = unix_time();
        append_jsonl(
            &journal_path(),
            &json!({
                "at": at,
                "session": session,
                "action": action.id(),
            }),
        );
        self.action_journal.push_back(JournalRecord {
            at,
            session,
            action: action.id().to_owned(),
        });
        while self.action_journal.len() > JOURNAL_TAIL_MAX {
            self.action_journal.pop_front();
        }
        self.resolve_pending_predictions(session, action);
    }

    /// A journaled action closes predictions scoped to its session: an
    /// exact candidate match is a hit, an `other` prediction hits on any
    /// action the question never listed, and a prompt send ends the
    /// window — fulfilling its own match and missing everything else.
    /// Actions that fulfill nothing leave the window open; quit censors
    /// silently by leaving the prediction unresolved.
    fn resolve_pending_predictions(&mut self, session: Option<Uuid>, action: JournalAction) {
        let Some(session) = session else {
            return;
        };
        let action_id = action.id();
        let window_ended = action.ends_prediction_window();
        let mut resolved: Vec<(Uuid, &'static str, bool)> = Vec::new();
        self.pending_action_predictions.retain(|prediction| {
            if prediction.session_id != session {
                return true;
            }
            let hit = action_id == prediction.predicted
                || (prediction.predicted == "other"
                    && !prediction
                        .candidates
                        .iter()
                        .any(|candidate| candidate == action_id));
            let outcome = if hit {
                Some("hit")
            } else if window_ended {
                Some("miss")
            } else {
                None
            };
            match outcome {
                Some(outcome) => {
                    resolved.push((prediction.id, outcome, prediction.adopted));
                    false
                }
                None => true,
            }
        });
        for (prediction_id, outcome, adopted) in resolved {
            if self
                .action_suggestion
                .as_ref()
                .is_some_and(|suggestion| suggestion.prediction_id == prediction_id)
            {
                self.action_suggestion = None;
            }
            append_jsonl(
                &prediction_log_path(),
                &json!({
                    "type": "resolution",
                    "prediction": prediction_id,
                    "at": unix_time(),
                    "outcome": outcome,
                    "action": action_id,
                    "adopted": adopted,
                }),
            );
        }
    }

    /// The session a workspace-bound action belongs to — the selected one
    /// first, then any session whose checkout is the path. `None` still
    /// journals (the prior is global); it just resolves nothing.
    pub(super) fn journal_session_for_workspace(&self, workspace: &Path) -> Option<Uuid> {
        let owns =
            |session: &AgentSession| self.workspace_path_for_session(session) == Some(workspace);
        self.state
            .selected_session
            .and_then(|id| self.state.sessions.iter().find(|session| session.id == id))
            .filter(|session| owns(session))
            .or_else(|| self.state.sessions.iter().find(|session| owns(session)))
            .map(|session| session.id)
    }

    /// Forget pending predictions and in-flight bookkeeping; the journal
    /// and prediction logs are the durable record and stay — toggling off
    /// only stops the feature's runtime state from going stale.
    pub(super) fn clear_action_predictions(&mut self) {
        self.pending_action_predictions.clear();
        self.pending_action_prediction_turns.clear();
        self.action_prediction_in_flight.clear();
        self.action_suggestion = None;
    }

    /// The chip's presentation: icon and localized label. `None` means the
    /// candidate is not renderable — call sites reach here only through
    /// `gated_suggestion`, which already filtered to actionable ids.
    fn suggestion_parts(&self, action: &str) -> Option<(&'static str, String)> {
        match action {
            "commit" => Some(("icons/git-commit-horizontal.svg", tr!("suggestions.commit"))),
            "push" => Some(("icons/arrow-up.svg", tr!("suggestions.push"))),
            "sync" => Some(("icons/arrow-down.svg", tr!("suggestions.sync"))),
            "land" => Some(("icons/git-merge.svg", tr!("suggestions.land"))),
            canned => {
                let prompt = suggested_prompt(canned, &self.state.suggested_prompts, None)?;
                Some(("icons/sparkle.svg", prompt))
            }
        }
    }

    /// Whether the suggestion row renders this frame, mirroring
    /// `render_action_suggestion`'s layering: a settled turn's status row
    /// claims the slot first — an empty `turn_status_suggestions` entry
    /// still claims it, suppressing the prediction fallback — and the
    /// predicted action chips in only when no entry does. Floats sharing
    /// the row's slot check this so the two never overlap.
    pub(super) fn action_suggestion_row_visible(&self) -> bool {
        let Some(session) = self.composer_session() else {
            return false;
        };
        if let Some(turn) = session.turns.last()
            && let Some(actions) = self.turn_status_suggestions.get(&turn.id)
        {
            return self.state.status_markers_enabled
                && turn.status == TurnStatus::Completed
                && !actions.is_empty();
        }
        self.action_suggestion.as_ref().is_some_and(|suggestion| {
            suggestion.session_id == session.id
                && self.suggestion_parts(&suggestion.action).is_some()
        })
    }

    /// The suggestion row floating above the composer lane. The row itself
    /// is a zero-height sibling at the lane's top — it takes no layout
    /// space, so a chip appearing or clearing never moves the composer or
    /// the transcript. The chip hangs off its bottom edge, painted over the
    /// transcript's bottom padding.
    pub(super) fn render_action_suggestion(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if self
            .composer_session()
            .and_then(|session| session.turns.last())
            .is_some_and(|turn| self.turn_status_suggestions.contains_key(&turn.id))
        {
            return self.render_status_suggestion(window, cx);
        }
        let suggestion = self.action_suggestion.as_ref()?;
        if self.composer_session().map(|session| session.id) != Some(suggestion.session_id) {
            return None;
        }
        let (icon_path, label) = self.suggestion_parts(suggestion.action)?;
        // ⌘⏎ fires the chip while the composer is empty — advertise the
        // chord exactly when it is bound, resolved as if the field were
        // focused (the binding lives on the TextInput context).
        let shortcut_label = if self.composer_is_empty(cx) {
            ShortcutHint::action_in(&crate::input::SubmitSteer, &self.composer_focus(cx))
                .resolve(window, cx)
        } else {
            None
        };
        let tooltip = label.clone();
        let display_label: String = label.replace('\n', " ").chars().take(96).collect();
        let display_label = if label.chars().count() > 96 || label.contains('\n') {
            format!("{display_label}…")
        } else {
            display_label
        };
        let theme = Theme::current(cx);
        // The turn's verdict pill leads the row while the footer holding its
        // inline copy sits below the fold — the standalone transcript float
        // yields to this row, so the two never occupy the same slot.
        let marker_pill =
            self.floating_status_marker_pill(self.active_transcript_rows(), &theme, cx);
        Some(
            div().w_full().h(px(0.0)).relative().child(
                div()
                    .absolute()
                    .bottom(px(8.0))
                    .left_0()
                    .right_0()
                    // Same insets and content width as the composer card
                    // below; the extra left inset lands the chip's edge on
                    // the project chip's icon in the workspace footer.
                    .px(px(20.0 - COMPOSER_OVERHANG))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(CONTENT_MAX_WIDTH + COMPOSER_OVERHANG * 2.0))
                            .mx_auto()
                            .pl(px(COMPOSER_CHIP_INSET))
                            .flex()
                            .gap(px(6.0))
                            .children(marker_pill)
                            .child(
                                div()
                                    .id("action-suggestion")
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
                                    .track_focus(&self.action_suggestion_focus)
                                    .tab_index(0)
                                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                                    .hover(|element| element.bg(theme.overlay_strong))
                                    .child(icon(icon_path, 11.0, theme.text_secondary))
                                    .child(display_label)
                                    .when_some(shortcut_label, |chip, label| {
                                        chip.child(
                                            div()
                                                .flex_none()
                                                .text_color(theme.text_tertiary)
                                                .child(label),
                                        )
                                    })
                                    .tooltip(Tooltip::text(tooltip))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.accept_action_suggestion(window, cx);
                                    }))
                                    .on_key_down(cx.listener(
                                        move |this, event: &KeyDownEvent, window, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                this.accept_action_suggestion(window, cx);
                                                cx.stop_propagation();
                                            }
                                        },
                                    )),
                            ),
                    ),
            ),
        )
    }

    /// Run the suggestion: mark the prediction adopted for the resolution
    /// log, then dispatch the same way the action's own affordance does —
    /// a canned prompt submits through the composer path, a Git action
    /// through the panel's op machinery.
    fn accept_action_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(suggestion) = self.action_suggestion.clone() else {
            return;
        };
        if let Some(prediction) = self
            .pending_action_predictions
            .iter_mut()
            .find(|prediction| prediction.id == suggestion.prediction_id)
        {
            prediction.adopted = true;
        }
        match suggestion_dispatch(suggestion.action) {
            Some(SuggestionDispatch::Prompt(id)) => {
                if let Some(prompt) = suggested_prompt(id, &self.state.suggested_prompts, None) {
                    self.submit_canned_prompt_to(suggestion.session_id, id, prompt, cx);
                }
            }
            Some(SuggestionDispatch::Commit) => self.open_commit_dialog(window, cx),
            Some(SuggestionDispatch::Land) => self.land_composer_session(PullStrategy::Rebase, cx),
            Some(dispatch) => {
                let workspace = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == suggestion.session_id)
                    .and_then(|session| {
                        self.workspace_path_for_session(session)
                            .map(Path::to_path_buf)
                    });
                if let Some(workspace) = workspace {
                    match dispatch {
                        SuggestionDispatch::Push => self.start_workspace_push(workspace, cx),
                        SuggestionDispatch::Sync => {
                            self.start_workspace_sync(workspace, PullStrategy::Rebase, cx)
                        }
                        _ => {}
                    }
                }
            }
            None => {}
        }
    }

    /// ⌘⏎ on an empty composer fires whatever the suggestion lane is
    /// showing, with the same layering `render_action_suggestion` draws:
    /// a settled turn's status row claims the slot first and its leftmost
    /// chip is the one the chord hits, the predicted action chips in only
    /// when no status row does. Returns whether a suggestion fired —
    /// `false` means the keystroke falls through to its usual empty-draft
    /// meaning.
    ///
    /// Dispatch defers a tick: the caller is a `ComposerEvent` subscription
    /// that already holds this entity mutably, and the accept paths need a
    /// `&mut Window` besides — both resolve once the notification returns.
    pub(super) fn accept_displayed_suggestion(&mut self, cx: &mut Context<Self>) -> bool {
        enum Displayed {
            Status(Uuid, StatusSuggestedAction),
            Action,
        }
        let displayed = self
            .composer_session()
            .and_then(|session| session.turns.last())
            .filter(|turn| {
                self.state.status_markers_enabled && turn.status == TurnStatus::Completed
            })
            .and_then(|turn| {
                self.turn_status_suggestions
                    .get(&turn.id)
                    .and_then(|actions| actions.first())
                    .map(|action| Displayed::Status(turn.id, action.clone()))
            });
        let displayed = match displayed {
            Some(displayed) => Some(displayed),
            // The status slot only yields to the prediction when no entry
            // claims it — same gate as the render path.
            None if self
                .composer_session()
                .and_then(|session| session.turns.last())
                .is_some_and(|turn| self.turn_status_suggestions.contains_key(&turn.id)) =>
            {
                None
            }
            None => self
                .action_suggestion
                .as_ref()
                .filter(|suggestion| {
                    self.composer_session().map(|session| session.id) == Some(suggestion.session_id)
                        && self.suggestion_parts(&suggestion.action).is_some()
                })
                .map(|_| Displayed::Action),
        };
        let Some(displayed) = displayed else {
            return false;
        };
        let waku = cx.entity();
        let window_handle = self.window_handle;
        cx.defer(move |cx| {
            let _ = window_handle.update(cx, move |_, window, cx| {
                let _ = waku.update(cx, |this, cx| match &displayed {
                    Displayed::Status(turn_id, action) => {
                        this.accept_status_suggestion(*turn_id, action, window, cx);
                    }
                    Displayed::Action => this.accept_action_suggestion(window, cx),
                });
            });
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use waku_protocol::model::{
        ActivityItem, ActivityKind, AgentTurn, Checkpoint, CheckpointFile, CheckpointStatus,
        ProviderKind, TranscriptBlock, TurnStatus,
    };

    use super::*;

    fn session_with_turn(checkpoint: Option<Checkpoint>) -> (AgentSession, Uuid) {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let turn_id = Uuid::new_v4();
        session.turns.push(AgentTurn {
            id: turn_id,
            turn_count: 1,
            status: TurnStatus::Completed,
            provider_turn_started: true,
            provider_resume_at: None,
            started_at: 1,
            completed_at: Some(2),
            checkpoint,
        });
        (session, turn_id)
    }

    fn candidate_ids(session: &AgentSession, turn_id: Uuid) -> Vec<&'static str> {
        next_action_candidates(session, turn_id)
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    #[test]
    fn canned_prompt_ids_match_the_submitted_text() {
        let defaults = BTreeMap::new();
        assert_eq!(
            canned_prompt_id("Run the tests", &defaults),
            Some("run-tests")
        );
        assert_eq!(
            canned_prompt_id("  keep going  ", &defaults),
            Some("keep-going")
        );
        assert_eq!(
            canned_prompt_id("KEEP GOING", &defaults),
            Some("keep-going")
        );
        assert_eq!(canned_prompt_id("run the tests please", &defaults), None);
        assert_eq!(canned_prompt_id("", &defaults), None);

        let overrides = BTreeMap::from([("run-tests".to_owned(), "Run focused tests".to_owned())]);
        assert_eq!(
            suggested_prompt("run-tests", &overrides, None).as_deref(),
            Some("Run focused tests")
        );
        assert_eq!(
            canned_prompt_id("run focused tests", &overrides),
            Some("run-tests")
        );
        assert_eq!(canned_prompt_id("Run the tests", &overrides), None);
        assert!(valid_suggested_prompt(CHOICE_PROMPT_ID, "Choose {option}"));
        assert!(!valid_suggested_prompt(CHOICE_PROMPT_ID, "Choose this"));
        assert_eq!(
            suggested_prompt(
                CHOICE_PROMPT_ID,
                &BTreeMap::from([(
                    CHOICE_PROMPT_ID.to_owned(),
                    "Select {option} and continue".to_owned()
                )]),
                Some("A. SQLite")
            )
            .as_deref(),
            Some("Select A. SQLite and continue")
        );
    }

    #[test]
    fn a_turn_without_changes_offers_no_work_candidates() {
        let (session, turn_id) = session_with_turn(None);
        let ids = candidate_ids(&session, turn_id);
        for gated in [
            "run-tests",
            "commit-changes",
            "add-tests",
            "review-changes",
            "open-pr",
            "commit",
            "push",
            "revert",
            "fix-errors",
        ] {
            assert!(!ids.contains(&gated), "unexpected candidate: {gated}");
        }
        for always in [
            "keep-going",
            "archive",
            "new-task",
            "new-worktree",
            "new-prompt",
        ] {
            assert!(ids.contains(&always), "missing candidate: {always}");
        }
    }

    #[test]
    fn a_turn_with_changes_offers_the_work_candidates() {
        let (session, turn_id) = session_with_turn(Some(Checkpoint {
            turn_count: 1,
            git_ref: "refs/waku/checkpoint".to_owned(),
            status: CheckpointStatus::Ready,
            files: vec![CheckpointFile {
                path: "src/lib.rs".to_owned(),
                additions: 3,
                deletions: 1,
            }],
            additions: 3,
            deletions: 1,
            created_at: 1,
        }));
        let ids = candidate_ids(&session, turn_id);
        for expected in ["run-tests", "commit-changes", "commit", "push", "revert"] {
            assert!(ids.contains(&expected), "missing candidate: {expected}");
        }
    }

    #[test]
    fn a_failed_activity_offers_fix_errors() {
        let (mut session, turn_id) = session_with_turn(None);
        let mut activity = ActivityItem::new(None, ActivityKind::Command, "cargo test", None, true);
        activity.failed = true;
        session.transcript_blocks.push(TranscriptBlock {
            after_message: 0,
            turn_id: Some(turn_id),
            activities: vec![activity],
        });
        assert!(candidate_ids(&session, turn_id).contains(&"fix-errors"));
    }

    #[test]
    fn the_suggestion_gate_wants_confidence_margin_and_an_actionable_pick() {
        let probabilities = |picks: &[(&str, f64)]| {
            picks
                .iter()
                .map(|(id, p)| (id.to_string(), *p))
                .collect::<BTreeMap<_, _>>()
        };
        // Confident, committed, and actionable — renders.
        let p = probabilities(&[("run-tests", 0.6), ("new-prompt", 0.4)]);
        assert_eq!(gated_suggestion("run-tests", &p), Some("run-tests"));
        // Confident but committed to a non-renderable pick — nothing.
        let p = probabilities(&[("archive", 0.8), ("run-tests", 0.2)]);
        assert_eq!(gated_suggestion("archive", &p), None);
        // Actionable but under the probability floor — nothing.
        let p = probabilities(&[("run-tests", 0.4), ("new-prompt", 0.6)]);
        assert_eq!(gated_suggestion("run-tests", &p), None);
        // Actionable and confident but too close to the runner-up.
        let p = probabilities(&[("run-tests", 0.55), ("commit", 0.45)]);
        assert_eq!(gated_suggestion("run-tests", &p), None);
    }

    #[test]
    fn a_worktree_session_offers_land() {
        let (mut session, turn_id) = session_with_turn(None);
        session.workspace = SessionWorkspace::Worktree {
            path: PathBuf::from("/tmp/worktree"),
            name: "worktree".to_owned(),
            branch: None,
            base_branch: None,
        };
        assert!(candidate_ids(&session, turn_id).contains(&"land"));
    }
}
