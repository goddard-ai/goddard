//! Phase-aware model routing: a session's lifecycle is tracked as
//! `Planning` → `Executing` and reported on the sidebar's quiet status
//! line. Auto-routed sessions whose intake judged the task plan-worthy
//! also retune the live model at the boundary — planning on the user's
//! hardest-class target, implementation one class tier below.
//!
//! The transition is deliberately cheap: every streamed tool event runs
//! through [`ActivityItem::phase_signal`], so reads, searches, and plan
//! tools keep a session in Planning while the first durable non-doc edit
//! commits it to Executing — no evaluation call. Writes that may be the
//! plan document itself, bare commands, and stalls stay ambiguous and
//! resolve at turn settle through the evaluation backend — never one
//! hosted call per tool event.
//!
//! Evaluations exist to move a model, so they only run for sessions the
//! intake route marked `phased` — unlike status markers, they are not
//! selection-gated, because the retune pays for itself on background
//! sessions too. Manual model picks clear `route_decision` and end
//! routing ownership; the marker keeps tracking the work either way.

use std::collections::BTreeMap;

use serde_json::json;
use uuid::Uuid;

use waku_protocol::eval::{EvalAnswer, EvalQuestion, Evaluation};
use waku_protocol::model::{ActivityItem, AgentSession, ProviderModel};
use waku_protocol::routing::{PhaseSignal, SessionPhase};

use super::*;

/// The decision-log feature tag phase evaluations record under, so the
/// calibration dataset keeps them distinct from intake `route` calls.
const PHASE_EVAL_FEATURE: &str = "route-phase";

/// `still_planning` at or above this probability means the agent is still
/// deciding — for an `Executing` session, that it went back to planning.
const STILL_PLANNING_ACTIVE: f64 = 0.6;
/// `still_planning` at or below this means planning is done: the session
/// commits to `Executing`. The band between the two thresholds changes
/// nothing — an undecided boundary keeps the planning model, which is the
/// safe side of the bet.
const STILL_PLANNING_DONE: f64 = 0.4;
/// `stuck` at or above this sends an `Executing` session back to Planning
/// and back up to the planning model.
const STUCK_THRESHOLD: f64 = 0.6;
/// The evaluator's implementation-model pick must clear this confidence
/// before it applies.
const MIN_IMPL_CONFIDENCE: f64 = 0.6;
/// Failed activities in one turn that justify a `stuck` check — below it,
/// errors are ordinary friction, not a stall worth an evaluation.
const STUCK_FAILURE_COUNT: usize = 2;

/// The icon and label a sidebar row shows for a known phase — `None` when
/// classification is off or the session's tool stream has not classified
/// yet. Icon and text both carry the meaning; color is only decoration.
pub(super) fn sidebar_phase_marker(
    enabled: bool,
    session: &AgentSession,
) -> Option<(&'static str, &'static str)> {
    if !enabled {
        return None;
    }
    Some(match session.phase? {
        SessionPhase::Planning => ("icons/target.svg", "phase.planning"),
        SessionPhase::Executing => ("icons/hammer.svg", "phase.executing"),
    })
}

/// What a settled turn's tool stream says about the phase boundary —
/// computed once per settle over that turn's activities only.
#[derive(Debug, Default)]
struct TurnPhaseScan {
    /// A command ran or a doc-shaped write landed while no durable edit
    /// committed — the boundary is live and undecided.
    ambiguous: bool,
    /// Failed activities this turn — the stuck heuristic's raw material.
    failures: usize,
    /// Every activity was exploration-shaped — reads, searches, plan
    /// tools — and the turn produced at least one. During `Executing`
    /// that means the agent went back to deciding.
    exploration_only: bool,
}

fn scan_turn_phase(session: &AgentSession, turn_id: Uuid) -> TurnPhaseScan {
    let mut scan = TurnPhaseScan::default();
    let mut saw_activity = false;
    let mut saw_execution = false;
    for activity in session
        .transcript_blocks
        .iter()
        .filter(|block| block.turn_id == Some(turn_id))
        .flat_map(|block| block.activities.iter())
    {
        saw_activity = true;
        if activity.failed {
            // A refusal or error is evidence for `stuck`, never for the
            // boundary — the agent decided nothing by being denied.
            scan.failures += 1;
            continue;
        }
        match activity.phase_signal() {
            PhaseSignal::Planning => {}
            PhaseSignal::Ambiguous => {
                saw_execution = true;
                scan.ambiguous = true;
            }
            PhaseSignal::Committing => saw_execution = true,
        }
    }
    scan.exploration_only = saw_activity && !saw_execution;
    scan
}

/// The eval's `implementation_model` question offers the session
/// provider's catalog plus this keep-what-it-has escape.
const CURRENT_MODEL_OPTION: &str = "current";

/// The three judgments one phase evaluation carries — independent
/// questions on one shared state, asked together so a settle costs one
/// call. `still_planning` answers both directions of the boundary;
/// `stuck` only matters while executing; `implementation_model` only
/// pays off when the phase actually commits.
fn phase_eval_questions(models: &[ProviderModel]) -> BTreeMap<String, EvalQuestion> {
    let mut criteria: BTreeMap<String, Option<String>> = models
        .iter()
        .map(|model| (model.id.clone(), Some(model.name.clone())))
        .collect();
    criteria.insert(
        CURRENT_MODEL_OPTION.to_owned(),
        Some(
            "keep the model the session already runs — pick it when no cheaper listed model can \
             plausibly execute the plan"
                .to_owned(),
        ),
    );
    BTreeMap::from([
        (
            "still_planning".to_owned(),
            EvalQuestion::Noul {
                instructions: "Is the agent still deciding what to do — exploring code, comparing \
                    approaches, or writing the plan itself — rather than executing a settled \
                    approach? Judge `toolSequence`, `filesChanged`, and the closing `response` \
                    together."
                    .to_owned(),
                criteria: None,
            },
        ),
        (
            "stuck".to_owned(),
            EvalQuestion::Noul {
                instructions: "Is implementation failing to make progress — repeated tool errors, \
                    the same failing approach retried, or evidence the plan is wrong? Answer high \
                    only when a stronger model could plausibly unstick it."
                    .to_owned(),
                criteria: None,
            },
        ),
        (
            "implementation_model".to_owned(),
            EvalQuestion::Choice {
                instructions: "The task is leaving its planning phase. Which listed model is the \
                    cheapest that can still execute this plan reliably? Judge the tool sequence \
                    and files changed — mechanical edits and test runs suit the cheapest option; \
                    subtle integration work may need more."
                    .to_owned(),
                criteria,
            },
        ),
    ])
}

fn noul_answer(evaluation: &Evaluation, key: &str) -> Option<f64> {
    match evaluation.answers.get(key) {
        Some(EvalAnswer::Noul { noul }) => Some(*noul),
        _ => None,
    }
}

/// The evaluator's implementation-model pick, gated on its confidence and
/// on actually choosing a model — `current` and missing answers mean "no
/// opinion to apply".
fn implementation_model_answer(evaluation: &Evaluation) -> Option<String> {
    match evaluation.answers.get("implementation_model") {
        Some(EvalAnswer::Choice {
            choice, confidence, ..
        }) if choice != CURRENT_MODEL_OPTION
            && confidence.unwrap_or(1.0) >= MIN_IMPL_CONFIDENCE =>
        {
            Some(choice.clone())
        }
        _ => None,
    }
}

/// What the evaluation concluded about the phase. Kept pure — the caller
/// supplies the phase the session was in so an answer that lands late
/// cannot relight a boundary the deterministic signal already passed.
enum PhaseVerdict {
    /// Planning is over — commit to Executing and downshift, possibly to
    /// the evaluator's model pick.
    Committed {
        implementation_model: Option<String>,
    },
    /// Implementation stalled or the agent went back to deciding —
    /// restore Planning and its model.
    Escalate,
}

fn phase_verdict(phase: SessionPhase, evaluation: &Evaluation) -> Option<PhaseVerdict> {
    match phase {
        SessionPhase::Planning => noul_answer(evaluation, "still_planning").and_then(|still| {
            (still <= STILL_PLANNING_DONE).then(|| PhaseVerdict::Committed {
                implementation_model: implementation_model_answer(evaluation),
            })
        }),
        SessionPhase::Executing => {
            let replanning = noul_answer(evaluation, "still_planning")
                .is_some_and(|p| p >= STILL_PLANNING_ACTIVE);
            let stuck = noul_answer(evaluation, "stuck").is_some_and(|p| p >= STUCK_THRESHOLD);
            (replanning || stuck).then_some(PhaseVerdict::Escalate)
        }
    }
}

/// Where a phased session's model goes at a phase change: provider, model
/// (`None` = the provider default), and effort.
struct PhaseTarget {
    model: Option<String>,
    effort: Option<String>,
}

impl Waku {
    pub(super) fn phase_classification_enabled(&self) -> bool {
        self.state.sidebar_phase_groups || self.state.phase_routing_enabled
    }

    /// Fold one streamed tool event into the session's phase when a feature
    /// needs it. A Committing signal flips Planning → Executing; phase-aware
    /// routing also retunes the model at that boundary. Ambiguous signals
    /// wait for a settle evaluation only when routing owns the task.
    pub(super) fn note_phase_activity(
        &mut self,
        session_id: Uuid,
        item: &ActivityItem,
        cx: &mut Context<Self>,
    ) {
        if !self.phase_classification_enabled() {
            return;
        }
        let (changed, committed) = {
            let Some(session) = self.state.session_mut(session_id) else {
                return;
            };
            let previous = session.phase;
            let phase = session.phase.get_or_insert(SessionPhase::Planning);
            if *phase == SessionPhase::Planning && item.phase_signal() == PhaseSignal::Committing {
                *phase = SessionPhase::Executing;
            }
            let changed = session.phase != previous;
            if changed {
                session.updated_at = unix_time();
            }
            (
                changed,
                session.phase == Some(SessionPhase::Executing) && changed,
            )
        };
        if !changed {
            return;
        }
        self.state.mark_session_dirty(session_id);
        cx.notify();
        if !committed || !self.state.phase_routing_enabled {
            return;
        }
        // The class map supplies the implementation model when it can;
        // when its tier entry points at another provider the evaluator
        // picks inside this provider's catalog instead — asked now so the
        // swap lands before the next turn.
        if !self.apply_phase_downshift(session_id, None, cx) {
            let turn_id = self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| {
                    session
                        .active_turn_id()
                        .or_else(|| session.turns.last().map(|turn| turn.id))
                });
            if let Some(turn_id) = turn_id {
                self.request_phase_eval(session_id, turn_id, None, cx);
            }
        }
    }

    /// A turn settling is when ambiguous phase state gets judged: a
    /// Planning session that ran ambiguous work asks whether planning
    /// ended; an Executing session with a stall signature asks whether to
    /// climb back. Deterministic boundaries need no call at all.
    pub(super) fn note_turn_finished_for_phase_eval(
        &mut self,
        session_id: Uuid,
        turn_id: Option<Uuid>,
        summary: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(turn_id) = turn_id else {
            return;
        };
        if !self.state.phase_routing_enabled {
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
        // Evaluations exist to move a model — only sessions whose intake
        // route declared a planning phase can spend them.
        if !session
            .route_decision
            .as_ref()
            .is_some_and(|decision| decision.phased)
        {
            return;
        }
        let scan = scan_turn_phase(session, turn_id);
        let needed = match session.phase {
            Some(SessionPhase::Planning) => scan.ambiguous,
            Some(SessionPhase::Executing) => {
                scan.failures >= STUCK_FAILURE_COUNT || scan.exploration_only
            }
            None => false,
        };
        if needed {
            self.request_phase_eval(session_id, turn_id, summary, cx);
        }
    }

    /// Build the turn's state and ask its session's daemon for the phase
    /// judgment, off the UI thread. An unusable eval backend means the
    /// feature simply does not fire — the marker still tracks the tool
    /// stream; only the model never moves.
    fn request_phase_eval(
        &mut self,
        session_id: Uuid,
        turn_id: Uuid,
        summary: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.state.phase_routing_enabled || self.phase_eval_in_flight.contains(&session_id) {
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
        if !session
            .route_decision
            .as_ref()
            .is_some_and(|decision| decision.phased)
        {
            return;
        }
        let models = self
            .provider_probe(session.provider)
            .map(|probe| probe.models.clone())
            .unwrap_or_default();
        // The marker's turn state plus the phase context the questions
        // judge against: where the session stands, what it runs, and the
        // catalog the implementation pick chooses from.
        let mut state = status_markers::turn_eval_state(session, turn_id, summary.as_deref());
        state["phase"] = json!(match session.phase {
            Some(SessionPhase::Planning) => "planning",
            Some(SessionPhase::Executing) => "executing",
            None => "unknown",
        });
        state["currentModel"] = json!(session.model);
        state["candidateModels"] = json!(
            models
                .iter()
                .map(|model| json!({ "id": model.id, "name": model.name }))
                .collect::<Vec<_>>()
        );
        let questions = phase_eval_questions(&models);
        let tx = self.phase_eval_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        self.phase_eval_in_flight.insert(session_id);
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
                            feature: Some(PHASE_EVAL_FEATURE.to_owned()),
                            timeout_secs: None,
                        },
                    )
                    .map_err(|error| format!("{error:#}"))
                    .and_then(|payload| match payload {
                        waku_client::ResponsePayload::Evaluation { evaluation } => Ok(evaluation),
                        _ => Err("the daemon returned an invalid evaluation response".to_owned()),
                    });
                if tx.send((session_id, result)).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .detach();
    }

    /// Land answered phase evaluations. A failed call drops quietly — the
    /// session keeps its current model and phase, matching every other
    /// eval fallback.
    pub(super) fn drain_phase_eval_events(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok((session_id, result)) = self.phase_eval_events.try_recv() {
            self.phase_eval_in_flight.remove(&session_id);
            match result {
                Ok(evaluation) if self.state.phase_routing_enabled => {
                    changed |= self.apply_phase_verdict(session_id, &evaluation, cx);
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("Goddard: phase evaluation failed: {error}");
                }
            }
        }
        changed
    }

    /// Apply one answered evaluation to the session's phase and model.
    /// The phase read happens now, not when the call was made — an answer
    /// landing after a deterministic flip simply finds nothing to do. A
    /// manual model pick clears `route_decision`, so a stale answer that
    /// lands after the user took the wheel finds nothing to do either.
    fn apply_phase_verdict(
        &mut self,
        session_id: Uuid,
        evaluation: &Evaluation,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(phase) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .and_then(|session| {
                session
                    .route_decision
                    .as_ref()
                    .is_some_and(|decision| decision.phased)
                    .then_some(session.phase)
                    .flatten()
            })
        else {
            return false;
        };
        match phase_verdict(phase, evaluation) {
            Some(PhaseVerdict::Committed {
                implementation_model,
            }) => {
                if let Some(session) = self.state.session_mut(session_id) {
                    session.phase = Some(SessionPhase::Executing);
                    session.updated_at = unix_time();
                    self.state.mark_session_dirty(session_id);
                }
                self.apply_phase_downshift(session_id, implementation_model, cx);
                true
            }
            Some(PhaseVerdict::Escalate) => {
                if let Some(session) = self.state.session_mut(session_id) {
                    session.phase = Some(SessionPhase::Planning);
                    session.updated_at = unix_time();
                    self.state.mark_session_dirty(session_id);
                }
                self.apply_phase_escalate(session_id, cx);
                true
            }
            // No phase change — but an Executing session whose class map
            // could not supply the implementation tier still wants the
            // evaluator's pick. `apply_phase_downshift` is idempotent: an
            // already-applied target no-ops, and no pick resolves to none.
            None if phase == SessionPhase::Executing => {
                self.apply_phase_downshift(session_id, implementation_model_answer(evaluation), cx);
                false
            }
            None => false,
        }
    }

    /// Swap a phased session onto its phase target through the same
    /// options path a manual model pick uses: the live driver absorbs
    /// what it can, and a provider that cannot restarts on its resume
    /// cursor at the next prompt. Returns whether a concrete target
    /// resolved — `false` means the class map and the evaluator both came
    /// up empty and the planning model stays.
    fn apply_phase_downshift(
        &mut self,
        session_id: Uuid,
        eval_pick: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return false;
        };
        let Some(target) = self.phase_implementation_target(session, eval_pick.as_deref()) else {
            return false;
        };
        self.apply_phase_options(session_id, target, cx);
        true
    }

    /// Restore the planning model — the route's own target, which the
    /// session provably started on. The provider matches by construction:
    /// a manual provider switch clears `route_decision`, so this path
    /// only ever runs inside the provider the route chose.
    fn apply_phase_escalate(&mut self, session_id: Uuid, cx: &mut Context<Self>) -> bool {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return false;
        };
        let Some(target) = session
            .route_decision
            .as_ref()
            .filter(|decision| decision.phased)
            .filter(|decision| decision.target.provider == session.provider)
            .map(|decision| PhaseTarget {
                model: decision.target.model.clone(),
                effort: decision.target.effort.clone(),
            })
        else {
            return false;
        };
        self.apply_phase_options(session_id, target, cx);
        true
    }

    /// The implementation target for a downshift: the class entry one tier
    /// below the task's difficulty when it names this provider, else the
    /// evaluator's pick from the session's own catalog. A class entry
    /// pointing elsewhere would mean a provider switch mid-session — phase
    /// routing never does that — so the eval covers that gap.
    fn phase_implementation_target(
        &self,
        session: &AgentSession,
        eval_pick: Option<&str>,
    ) -> Option<PhaseTarget> {
        let provider = session.provider;
        let catalog_has = |model: &str| {
            self.provider_probe(provider)
                .is_none_or(|probe| probe.models.is_empty() || probe.model(model).is_some())
        };
        let decision = session
            .route_decision
            .as_ref()
            .filter(|decision| decision.phased)?;
        let implementation_class = decision.class.unwrap_or_default().implementation_class();
        if let Some(entry) = self
            .state
            .route_classes
            .get(&implementation_class)
            .filter(|entry| entry.provider == provider)
        {
            // Mirror the route's resolve_entry: a configured model missing
            // from the catalog drops to the provider default and cannot
            // carry its effort.
            return match entry.model.as_deref() {
                Some(model) if catalog_has(model) => Some(PhaseTarget {
                    model: Some(model.to_owned()),
                    effort: entry.effort.clone(),
                }),
                Some(_) => Some(PhaseTarget {
                    model: None,
                    effort: None,
                }),
                None => Some(PhaseTarget {
                    model: None,
                    effort: entry.effort.clone(),
                }),
            };
        }
        eval_pick
            .filter(|pick| catalog_has(pick))
            .map(|pick| PhaseTarget {
                model: Some(pick.to_owned()),
                effort: waku_client::persistence::remembered_model_traits_for(
                    self.state.remembered_model_traits(),
                    provider,
                    pick,
                )
                .0,
            })
    }

    /// Write a phase target onto the session and retune the live driver.
    /// Effort falls back to the model's remembered traits; a provider
    /// default (`None` model) leaves tier and window alone since no named
    /// model owns remembered values to restore.
    fn apply_phase_options(
        &mut self,
        session_id: Uuid,
        target: PhaseTarget,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        let provider = session.provider;
        let (remembered_effort, remembered_tier, remembered_window) = target
            .model
            .as_deref()
            .map(|model| {
                waku_client::persistence::remembered_model_traits_for(
                    self.state.remembered_model_traits(),
                    provider,
                    model,
                )
            })
            .unwrap_or_default();
        let effort = target.effort.or(remembered_effort);
        let (tier, window) = if target.model.is_some() {
            (remembered_tier, remembered_window)
        } else {
            (session.service_tier.clone(), session.context_window.clone())
        };
        if session.model == target.model
            && session.reasoning_effort == effort
            && session.service_tier == tier
            && session.context_window == window
        {
            return;
        }
        let Some(session) = self.state.session_mut(session_id) else {
            return;
        };
        session.model = target.model;
        session.reasoning_effort = effort;
        session.service_tier = tier;
        session.context_window = window;
        session.updated_at = unix_time();
        self.state.mark_session_dirty(session_id);
        self.apply_session_options(session_id, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::model::{ActivityFileChange, ActivityKind, MessageRole, TranscriptBlock};

    fn activity(kind: ActivityKind, target: Option<&str>, failed: bool) -> ActivityItem {
        let mut item = ActivityItem::new(None, kind, "Edit", target.map(str::to_owned), true);
        item.display_target = target.map(str::to_owned);
        item.failed = failed;
        item
    }

    fn file_change(path: &str) -> ActivityItem {
        let mut item = ActivityItem::new(None, ActivityKind::FileChange, "Edit", None, true);
        item.file_changes = vec![ActivityFileChange {
            path: path.to_owned(),
            additions: None,
            deletions: None,
            status: None,
            diff: None,
        }];
        item
    }

    fn turn_block(turn_id: Uuid, activities: Vec<ActivityItem>) -> TranscriptBlock {
        TranscriptBlock {
            after_message: 0,
            turn_id: Some(turn_id),
            activities,
        }
    }

    #[test]
    fn file_changes_to_code_commit_the_boundary() {
        assert_eq!(
            file_change("src/main.rs").phase_signal(),
            PhaseSignal::Committing
        );
    }

    #[test]
    fn doc_only_writes_stay_ambiguous() {
        for target in ["PLAN.md", "docs/design.md", "notes.txt", "spec-api.rst"] {
            assert_eq!(
                file_change(target).phase_signal(),
                PhaseSignal::Ambiguous,
                "{target} should read as planning output"
            );
        }
        // A write that touches the plan and real code together commits.
        let mut mixed = file_change("plan.md");
        mixed
            .file_changes
            .push(file_change("src/lib.rs").file_changes.pop().unwrap());
        assert_eq!(mixed.phase_signal(), PhaseSignal::Committing);
    }

    #[test]
    fn commands_are_ambiguous_and_reads_are_planning() {
        assert_eq!(
            activity(ActivityKind::Command, Some("cargo test"), false).phase_signal(),
            PhaseSignal::Ambiguous
        );
        assert_eq!(
            activity(ActivityKind::FileRead, Some("src/main.rs"), false).phase_signal(),
            PhaseSignal::Planning
        );
        assert_eq!(
            activity(ActivityKind::Plan, Some("steps"), false).phase_signal(),
            PhaseSignal::Planning
        );
    }

    #[test]
    fn failed_activity_never_commits() {
        assert_eq!(
            file_change("src/main.rs").with_failed(true).phase_signal(),
            PhaseSignal::Ambiguous
        );
    }

    #[test]
    fn scan_reports_ambiguous_and_stall_signatures() {
        let turn_id = Uuid::new_v4();
        let mut session =
            AgentSession::new(Uuid::new_v4(), waku_protocol::model::ProviderKind::Claude);
        session.transcript_blocks = vec![
            turn_block(
                turn_id,
                vec![activity(ActivityKind::FileRead, Some("a.rs"), false)],
            ),
            turn_block(
                turn_id,
                vec![activity(ActivityKind::Command, Some("ls"), false)],
            ),
        ];
        let scan = scan_turn_phase(&session, turn_id);
        assert!(scan.ambiguous);
        assert!(!scan.exploration_only);
        assert_eq!(scan.failures, 0);

        let other_turn = Uuid::new_v4();
        session.transcript_blocks = vec![turn_block(
            other_turn,
            vec![
                activity(ActivityKind::FileRead, Some("a.rs"), false),
                activity(ActivityKind::FileSearch, Some("query"), false),
            ],
        )];
        let scan = scan_turn_phase(&session, other_turn);
        assert!(!scan.ambiguous);
        assert!(scan.exploration_only);
    }

    #[test]
    fn planning_verdict_commits_below_the_done_threshold() {
        let evaluation = |still_planning: f64| Evaluation {
            model: "jev".to_owned(),
            answers: BTreeMap::from([(
                "still_planning".to_owned(),
                EvalAnswer::Noul {
                    noul: still_planning,
                },
            )]),
            usage: Default::default(),
            latency_ms: 0,
            provider_metadata: None,
        };
        assert!(matches!(
            phase_verdict(SessionPhase::Planning, &evaluation(0.2)),
            Some(PhaseVerdict::Committed { .. })
        ));
        // The dead band changes nothing — an undecided boundary keeps the
        // planning model.
        assert!(phase_verdict(SessionPhase::Planning, &evaluation(0.5)).is_none());
        assert!(phase_verdict(SessionPhase::Planning, &evaluation(0.8)).is_none());
    }

    #[test]
    fn executing_verdict_escalates_on_replan_or_stuck() {
        let evaluation = |still: f64, stuck: f64| Evaluation {
            model: "jev".to_owned(),
            answers: BTreeMap::from([
                (
                    "still_planning".to_owned(),
                    EvalAnswer::Noul { noul: still },
                ),
                ("stuck".to_owned(), EvalAnswer::Noul { noul: stuck }),
            ]),
            usage: Default::default(),
            latency_ms: 0,
            provider_metadata: None,
        };
        assert!(matches!(
            phase_verdict(SessionPhase::Executing, &evaluation(0.8, 0.1)),
            Some(PhaseVerdict::Escalate)
        ));
        assert!(matches!(
            phase_verdict(SessionPhase::Executing, &evaluation(0.2, 0.9)),
            Some(PhaseVerdict::Escalate)
        ));
        assert!(phase_verdict(SessionPhase::Executing, &evaluation(0.2, 0.1)).is_none());
        // A settled Executing session never commits again.
        assert!(phase_verdict(SessionPhase::Executing, &evaluation(0.1, 0.0)).is_none());
    }

    #[test]
    fn implementation_pick_needs_confidence_and_a_real_model() {
        let evaluation = |choice: &str, confidence: f64| Evaluation {
            model: "jev".to_owned(),
            answers: BTreeMap::from([(
                "implementation_model".to_owned(),
                EvalAnswer::Choice {
                    choice: choice.to_owned(),
                    confidence: Some(confidence),
                    probabilities: BTreeMap::new(),
                },
            )]),
            usage: Default::default(),
            latency_ms: 0,
            provider_metadata: None,
        };
        assert_eq!(
            implementation_model_answer(&evaluation("haiku", 0.9)).as_deref(),
            Some("haiku")
        );
        assert_eq!(implementation_model_answer(&evaluation("haiku", 0.3)), None);
        assert_eq!(
            implementation_model_answer(&evaluation("current", 0.9)),
            None
        );
    }

    #[test]
    fn marker_only_renders_for_known_phases() {
        let mut session =
            AgentSession::new(Uuid::new_v4(), waku_protocol::model::ProviderKind::Claude);
        assert_eq!(sidebar_phase_marker(true, &session), None);
        session.phase = Some(SessionPhase::Planning);
        assert_eq!(
            sidebar_phase_marker(true, &session),
            Some(("icons/target.svg", "phase.planning"))
        );
        session.phase = Some(SessionPhase::Executing);
        assert_eq!(
            sidebar_phase_marker(true, &session),
            Some(("icons/hammer.svg", "phase.executing"))
        );
        assert_eq!(sidebar_phase_marker(false, &session), None);
    }

    #[test]
    fn truncation_rederives_phase_from_surviving_activities() {
        let mut session =
            AgentSession::new(Uuid::new_v4(), waku_protocol::model::ProviderKind::Claude);
        let turn_one = Uuid::new_v4();
        let turn_two = Uuid::new_v4();
        let turn = |id| waku_protocol::model::AgentTurn {
            id,
            turn_count: 1,
            status: waku_protocol::model::TurnStatus::Completed,
            provider_turn_started: true,
            provider_resume_at: None,
            started_at: unix_time(),
            completed_at: Some(unix_time()),
            checkpoint: None,
        };
        session.turns = vec![turn(turn_one), turn(turn_two)];
        session
            .messages
            .push(waku_protocol::model::Message::new_for_turn(
                MessageRole::Assistant,
                "done",
                turn_two,
            ));
        // Turn one explored, turn two edited — the session earned
        // Executing. Rewinding to turn one drops the edit's block, and the
        // phase follows the surviving evidence back to Planning.
        session.transcript_blocks = vec![
            turn_block(
                turn_one,
                vec![activity(ActivityKind::FileRead, Some("a.rs"), false)],
            ),
            turn_block(turn_two, vec![file_change("src/main.rs")]),
        ];
        session.phase = Some(SessionPhase::Executing);
        session.truncate_after_turn(1);
        assert_eq!(session.phase, Some(SessionPhase::Planning));
    }
}
