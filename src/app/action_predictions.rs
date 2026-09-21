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
use waku_protocol::eval::{EvalAnswer, EvalQuestion, Evaluation};
use waku_protocol::model::{AgentSession, SessionWorkspace};

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

/// The fixed follow-up prompts a suggestion could send verbatim. `id` is
/// the journal's `canned` value and the candidate key; `text` is the exact
/// composer payload, so hand-typed equivalents match by comparison.
const CANNED_PROMPTS: &[(&str, &str)] = &[
    ("keep-going", "Keep going"),
    ("run-tests", "Run the tests"),
    ("fix-errors", "Fix the remaining errors"),
    ("commit-changes", "Commit these changes"),
    ("add-tests", "Add tests for this"),
    ("review-changes", "Review these changes for issues"),
    ("open-pr", "Push and open a PR"),
];

/// The canned id for a submitted prompt, when its text is one of the fixed
/// follow-ups — case- and whitespace-insensitive, so a hand-typed
/// equivalent counts as the same outcome.
pub(super) fn canned_prompt_id(prompt: &str) -> Option<&'static str> {
    let normalized = prompt.trim();
    CANNED_PROMPTS
        .iter()
        .find(|(_, text)| text.eq_ignore_ascii_case(normalized))
        .map(|(id, _)| *id)
}

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
        ("keep-going", "Send the follow-up prompt \"Keep going\""),
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
            ("run-tests", "Send the follow-up prompt \"Run the tests\""),
            (
                "commit-changes",
                "Send the follow-up prompt \"Commit these changes\"",
            ),
            (
                "add-tests",
                "Send the follow-up prompt \"Add tests for this\"",
            ),
            (
                "review-changes",
                "Send the follow-up prompt \"Review these changes for issues\"",
            ),
            (
                "open-pr",
                "Send the follow-up prompt \"Push and open a PR\"",
            ),
            ("commit", "Commit the changes through the Git panel"),
            ("push", "Push the work to its remote"),
            ("revert", "Rewind the workspace to before this turn"),
        ]);
    }
    if saw_failure {
        candidates.push((
            "fix-errors",
            "Send the follow-up prompt \"Fix the remaining errors\"",
        ));
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
    /// prediction, off the UI thread. An unconfigured eval backend means
    /// the feature simply does not fire.
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
        };
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
        let mut resolved: Vec<(Uuid, &'static str)> = Vec::new();
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
                    resolved.push((prediction.id, outcome));
                    false
                }
                None => true,
            }
        });
        for (prediction_id, outcome) in resolved {
            append_jsonl(
                &prediction_log_path(),
                &json!({
                    "type": "resolution",
                    "prediction": prediction_id,
                    "at": unix_time(),
                    "outcome": outcome,
                    "action": action_id,
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
        assert_eq!(canned_prompt_id("Run the tests"), Some("run-tests"));
        assert_eq!(canned_prompt_id("  keep going  "), Some("keep-going"));
        assert_eq!(canned_prompt_id("KEEP GOING"), Some("keep-going"));
        assert_eq!(canned_prompt_id("run the tests please"), None);
        assert_eq!(canned_prompt_id(""), None);
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
