//! New-session model routing: evaluate the first prompt into a task class,
//! then resolve the user's class map against the app's eligible candidate
//! set to a concrete provider/model/effort.
//!
//! The evaluator only names the kind of work — it never sees model ids and
//! its answers are untrusted input. Provider eligibility, the confidence
//! floor, and the final target all come from ordinary deterministic code in
//! this module, and every decision records the reason chain that produced
//! it.

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::{Value, json};
use waku_protocol::eval::{EvalAnswer, EvalQuestion, EvalSettings, Evaluation};
use waku_protocol::model::ProviderKind;
use waku_protocol::routing::{
    RouteCandidate, RouteClassMap, RouteClassTarget, RouteDecision, RouteTarget, TaskClass,
};

use crate::eval::EvalDecisionRecord;

/// One routing pass: the decision for the wire plus the log record that
/// explains it.
pub struct RouteRun {
    pub decision: RouteDecision,
    pub record: EvalDecisionRecord,
}

const CLASS_INSTRUCTIONS: &str = "Which model tier does this task require? medium is the user's \
workhorse and the default for ordinary engineering: exploration, debugging, code review, planning, \
and feature implementation, including multi-step work. Choose easy only for clearly mechanical, \
low-risk work with an obvious approach and little judgment. Choose hard only for unusually \
demanding reasoning, subtle interacting constraints, or high-stakes decisions where an ordinary \
capable model is unlikely to be reliable. Needing exploration, a plan, or multiple steps does not \
by itself warrant hard. When the evidence for either extreme is unclear, choose medium.";

/// The class answer must clear this confidence before it routes; below it
/// the session keeps the default route.
const MIN_CLASS_CONFIDENCE: f64 = 0.55;

const NEEDS_PLANNING_INSTRUCTIONS: &str = "Does this task warrant a distinct planning phase — \
exploring unfamiliar code, weighing approaches, or decomposing work — before implementation? \
Answer high for ambiguous, multi-step, or design-sensitive tasks; low for mechanical or \
single-step work whose approach is already obvious.";

/// The planning answer must clear this probability before the session is
/// marked phased; below it the task runs its class target end to end.
const MIN_PLANNING_PROBABILITY: f64 = 0.6;

/// Route one new-session submission. Never fails hard: every degraded path
/// still returns a usable target with the reason recorded.
pub fn route_task(
    eval_settings: Option<&EvalSettings>,
    classes: &RouteClassMap,
    prompt: &str,
    project: Option<&str>,
    candidates: &[RouteCandidate],
    last_used: Option<&RouteTarget>,
) -> RouteRun {
    route_task_with_evaluator(
        eval_settings,
        classes,
        prompt,
        project,
        candidates,
        last_used,
        crate::eval::evaluate,
    )
}

fn route_task_with_evaluator(
    eval_settings: Option<&EvalSettings>,
    classes: &RouteClassMap,
    prompt: &str,
    project: Option<&str>,
    candidates: &[RouteCandidate],
    last_used: Option<&RouteTarget>,
    evaluate: impl FnOnce(
        &EvalSettings,
        &Value,
        &BTreeMap<String, EvalQuestion>,
    ) -> anyhow::Result<Evaluation>,
) -> RouteRun {
    let started = Instant::now();
    let mut record = EvalDecisionRecord::empty("route");
    // What the classifier contributed, applied onto whichever target the
    // resolution produced — including fallback targets.
    let mut class_answered: Option<TaskClass> = None;
    let mut class_applied: Option<TaskClass> = None;
    let mut class_confidence: Option<f64> = None;
    // Whether the task earned a planning phase — applied onto whichever
    // resolution path ran, including fallbacks that skip the eval entirely.
    let mut phased = false;

    let (target, reasons) = 'resolve: {
        if candidates.is_empty() {
            break 'resolve (
                last_used.cloned().unwrap_or_else(|| RouteTarget {
                    provider: ProviderKind::default(),
                    model: None,
                    effort: None,
                }),
                vec!["no-candidates"],
            );
        }
        let default = |reasons: Vec<&'static str>| (default_target(candidates, last_used), reasons);
        let Some(eval_settings) = eval_settings else {
            break 'resolve default(vec!["eval-unconfigured"]);
        };
        record.backend = Some(eval_settings.provider);

        let state = json!({ "task": prompt, "project": project });
        let questions = routing_questions();
        record.state = Some(state.clone());
        record.questions = Some(questions.clone());

        let evaluation = match evaluate(eval_settings, &state, &questions) {
            Ok(evaluation) => evaluation,
            Err(error) => {
                record.error = Some(error.to_string());
                break 'resolve default(vec!["eval-failed"]);
            }
        };
        record.latency_ms = Some(evaluation.latency_ms);
        record.model = Some(evaluation.model.clone());
        record.usage = Some(evaluation.usage.clone());
        record.answers = Some(evaluation.answers.clone());

        let answer = choice_answer(&evaluation, "class");
        class_answered = answer
            .as_ref()
            .and_then(|(choice, _)| parse_class_answer(choice));
        class_confidence = answer.and_then(|(_, confidence)| confidence);
        let Some(class) = class_answered else {
            break 'resolve default(vec!["eval-missing-answer"]);
        };
        if class_confidence.unwrap_or(1.0) < MIN_CLASS_CONFIDENCE {
            break 'resolve default(vec!["low-class-confidence"]);
        }

        phased = matches!(
            evaluation.answers.get("needs_planning"),
            Some(EvalAnswer::Noul { noul }) if *noul >= MIN_PLANNING_PROBABILITY
        );
        // Planning is a workflow decision, not evidence that the work
        // requires the Hard model. Let the difficulty answer own the tier.
        match classes.get(&class) {
            Some(entry) => {
                let (target, note) = resolve_entry(entry, candidates, last_used);
                class_applied = Some(class);
                let mut reasons = vec!["class-map"];
                reasons.extend(note);
                (target, reasons)
            }
            None => default(vec!["class-unmapped"]),
        }
    };

    let decision = RouteDecision {
        target,
        class: class_answered,
        applied_class: class_applied,
        class_confidence,
        phased,
        reason: reasons.join("+"),
        backend: eval_settings.map(|settings| settings.provider),
        eval_latency_ms: Some(started.elapsed().as_millis() as u64),
    };
    record.complete(&decision);
    RouteRun { decision, record }
}

/// The question every route asks, shared so the decision log can reconstruct
/// exactly what the classifier saw.
pub fn routing_questions() -> BTreeMap<String, EvalQuestion> {
    BTreeMap::from([
        (
            "class".to_owned(),
            EvalQuestion::Choice {
                instructions: CLASS_INSTRUCTIONS.to_owned(),
                criteria: BTreeMap::from([
                    (
                        "easy".to_owned(),
                        Some("clearly mechanical, low-risk work with little judgment".to_owned()),
                    ),
                    (
                        "medium".to_owned(),
                        Some("the workhorse default: ordinary engineering, exploration, debugging, planning, and implementation".to_owned()),
                    ),
                    (
                        "hard".to_owned(),
                        Some("unusually demanding reasoning or high-stakes decisions beyond ordinary engineering".to_owned()),
                    ),
                ]),
            },
        ),
        (
            "needs_planning".to_owned(),
            EvalQuestion::Noul {
                instructions: NEEDS_PLANNING_INSTRUCTIONS.to_owned(),
                criteria: None,
            },
        ),
    ])
}

fn choice_answer(evaluation: &Evaluation, key: &str) -> Option<(String, Option<f64>)> {
    match evaluation.answers.get(key) {
        Some(EvalAnswer::Choice {
            choice, confidence, ..
        }) => Some((choice.clone(), *confidence)),
        _ => None,
    }
}

fn parse_class_answer(choice: &str) -> Option<TaskClass> {
    serde_json::from_value(Value::String(choice.to_owned())).ok()
}

fn find_candidate(
    candidates: &[RouteCandidate],
    provider: ProviderKind,
) -> Option<&RouteCandidate> {
    candidates
        .iter()
        .find(|candidate| candidate.provider == provider)
}

/// A concrete model id is usable when it appears in the provider's live
/// catalog — or the catalog is unknown (`models` empty), in which case the
/// configured id is accepted.
fn model_eligible(candidate: &RouteCandidate, model: &str) -> bool {
    candidate.models.is_empty() || candidate.models.iter().any(|m| m == model)
}

/// Resolve a configured class entry to an eligible route: the entry's own
/// provider/model/effort when everything is reachable, the provider's
/// default model when the configured model is not in its catalog, and the
/// default route when the provider cannot host the session at all.
fn resolve_entry(
    entry: &RouteClassTarget,
    candidates: &[RouteCandidate],
    last_used: Option<&RouteTarget>,
) -> (RouteTarget, Option<&'static str>) {
    let Some(candidate) = find_candidate(candidates, entry.provider) else {
        return (
            default_target(candidates, last_used),
            Some("provider-ineligible"),
        );
    };
    match &entry.model {
        Some(model) if model_eligible(candidate, model) => (
            RouteTarget {
                provider: entry.provider,
                model: Some(model.clone()),
                effort: entry.effort.clone(),
            },
            None,
        ),
        Some(_) => (
            RouteTarget {
                provider: entry.provider,
                model: None,
                effort: None,
            },
            Some("model-ineligible"),
        ),
        // A provider-default entry keeps its effort: the effort belongs to
        // whatever model the provider launches, which the user configured.
        None => (
            RouteTarget {
                provider: entry.provider,
                model: None,
                effort: entry.effort.clone(),
            },
            None,
        ),
    }
}

/// The default route: `last_used` when its provider is still a candidate
/// (its model and effort survive only while the catalog still lists them),
/// else the first candidate's provider default.
fn default_target(candidates: &[RouteCandidate], last_used: Option<&RouteTarget>) -> RouteTarget {
    if let Some(last) = last_used
        && let Some(candidate) = find_candidate(candidates, last.provider)
    {
        let model = last
            .model
            .as_deref()
            .filter(|model| model_eligible(candidate, model))
            .map(str::to_owned);
        // The effort belongs to the model it was chosen for — a fallback to
        // the provider default cannot carry it.
        let effort = model.as_ref().and_then(|_| last.effort.clone());
        return RouteTarget {
            provider: last.provider,
            model,
            effort,
        };
    }
    candidates
        .first()
        .map(|candidate| RouteTarget {
            provider: candidate.provider,
            model: None,
            effort: None,
        })
        .unwrap_or_else(|| RouteTarget {
            provider: ProviderKind::default(),
            model: None,
            effort: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(provider: ProviderKind, models: &[&str]) -> RouteCandidate {
        RouteCandidate {
            provider,
            models: models.iter().map(|model| model.to_string()).collect(),
        }
    }

    fn entry(
        provider: ProviderKind,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> RouteClassTarget {
        RouteClassTarget {
            provider,
            model: model.map(str::to_owned),
            effort: effort.map(str::to_owned),
        }
    }

    fn classes(pairs: &[(TaskClass, RouteClassTarget)]) -> RouteClassMap {
        pairs.iter().cloned().collect()
    }

    fn evaluation(class: TaskClass, confidence: f64, planning: f64) -> Evaluation {
        Evaluation {
            model: "test-evaluator".into(),
            answers: BTreeMap::from([
                (
                    "class".into(),
                    EvalAnswer::Choice {
                        choice: class.id().into(),
                        confidence: Some(confidence),
                        probabilities: BTreeMap::from([(class.id().into(), confidence)]),
                    },
                ),
                ("needs_planning".into(), EvalAnswer::Noul { noul: planning }),
            ]),
            usage: Default::default(),
            latency_ms: 1,
            provider_metadata: None,
        }
    }

    #[test]
    fn planning_keeps_the_class_model_and_logs_that_class() {
        let classes = classes(&[
            (
                TaskClass::Routine,
                entry(ProviderKind::Codex, Some("easy-model"), None),
            ),
            (
                TaskClass::General,
                entry(ProviderKind::Codex, Some("workhorse-model"), Some("medium")),
            ),
            (
                TaskClass::Demanding,
                entry(ProviderKind::Codex, Some("hard-model"), Some("high")),
            ),
        ]);
        let candidates = [candidate(
            ProviderKind::Codex,
            &["easy-model", "workhorse-model", "hard-model"],
        )];
        for class in waku_protocol::routing::ALL_TASK_CLASSES {
            for planning in [0.0, 0.99] {
                let run = route_task_with_evaluator(
                    Some(&EvalSettings::default()),
                    &classes,
                    "explore the code, plan the change, and implement it",
                    None,
                    &candidates,
                    None,
                    |_, _, _| Ok(evaluation(class, 0.9, planning)),
                );
                let expected = classes.get(&class).unwrap();
                assert_eq!(run.decision.target.model, expected.model);
                assert_eq!(run.decision.target.effort, expected.effort);
                assert_eq!(run.decision.class, Some(class));
                assert_eq!(run.decision.applied_class, Some(class));
                assert_eq!(run.decision.phased, planning >= MIN_PLANNING_PROBABILITY);
                assert_eq!(run.decision.reason, "class-map");
                let logged = serde_json::to_value(&run.record).unwrap();
                assert_eq!(logged["class"], class.id());
                assert_eq!(logged["appliedClass"], class.id());
                assert_eq!(logged["reason"], "class-map");
            }
        }
    }

    #[test]
    fn unmapped_medium_planning_does_not_borrow_the_hard_model() {
        let classes = classes(&[(
            TaskClass::Demanding,
            entry(ProviderKind::Codex, Some("hard-model"), Some("high")),
        )]);
        let candidates = [candidate(
            ProviderKind::Codex,
            &["workhorse-model", "hard-model"],
        )];
        let last_used = RouteTarget {
            provider: ProviderKind::Codex,
            model: Some("workhorse-model".into()),
            effort: Some("medium".into()),
        };
        let run = route_task_with_evaluator(
            Some(&EvalSettings::default()),
            &classes,
            "plan an ordinary feature",
            None,
            &candidates,
            Some(&last_used),
            |_, _, _| Ok(evaluation(TaskClass::General, 0.9, 0.99)),
        );
        assert_eq!(run.decision.target, last_used);
        assert!(run.decision.phased);
        assert_eq!(run.decision.applied_class, None);
        assert_eq!(run.decision.reason, "class-unmapped");
    }

    #[test]
    fn unconfigured_eval_falls_back_to_last_used() {
        let classes = RouteClassMap::new();
        let candidates = [
            candidate(ProviderKind::Claude, &["claude-sonnet-5"]),
            candidate(ProviderKind::Codex, &["gpt-5.5"]),
        ];
        let last_used = RouteTarget {
            provider: ProviderKind::Codex,
            model: Some("gpt-5.5".into()),
            effort: Some("high".into()),
        };
        let run = route_task(
            None,
            &classes,
            "fix the typo",
            None,
            &candidates,
            Some(&last_used),
        );
        assert_eq!(run.decision.target.provider, ProviderKind::Codex);
        assert_eq!(run.decision.target.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(run.decision.target.effort.as_deref(), Some("high"));
        assert!(run.decision.reason.contains("eval-unconfigured"));
    }

    #[test]
    fn unconfigured_eval_without_last_used_picks_first_candidate() {
        let classes = RouteClassMap::new();
        let candidates = [
            candidate(ProviderKind::Codex, &["gpt-5.5"]),
            candidate(ProviderKind::Claude, &["claude-sonnet-5"]),
        ];
        let run = route_task(None, &classes, "fix the typo", None, &candidates, None);
        assert_eq!(run.decision.target.provider, ProviderKind::Codex);
        assert_eq!(run.decision.target.model, None);
        assert_eq!(run.decision.target.effort, None);
    }

    #[test]
    fn ineligible_last_used_model_drops_to_provider_default() {
        let classes = RouteClassMap::new();
        let candidates = [candidate(ProviderKind::Claude, &["claude-sonnet-5"])];
        let last_used = RouteTarget {
            provider: ProviderKind::Claude,
            model: Some("claude-opus-5".into()),
            effort: Some("max".into()),
        };
        let run = route_task(None, &classes, "hello", None, &candidates, Some(&last_used));
        assert_eq!(run.decision.target.provider, ProviderKind::Claude);
        assert_eq!(run.decision.target.model, None);
        assert_eq!(
            run.decision.target.effort, None,
            "an effort cannot outlive the model it belongs to"
        );
    }

    #[test]
    fn empty_candidates_still_returns_a_target() {
        let classes = RouteClassMap::new();
        let run = route_task(None, &classes, "hello", None, &[], None);
        assert_eq!(run.decision.reason, "no-candidates");
    }

    #[test]
    fn class_entry_resolves_provider_model_and_effort() {
        let classes = classes(&[(
            TaskClass::Routine,
            entry(ProviderKind::Claude, Some("claude-haiku-4-5"), Some("low")),
        )]);
        let candidates = [
            candidate(
                ProviderKind::Claude,
                &["claude-haiku-4-5", "claude-sonnet-5"],
            ),
            candidate(ProviderKind::Codex, &["gpt-5.5"]),
        ];
        let (target, note) =
            resolve_entry(classes.get(&TaskClass::Routine).unwrap(), &candidates, None);
        assert_eq!(note, None);
        assert_eq!(target.provider, ProviderKind::Claude);
        assert_eq!(target.model.as_deref(), Some("claude-haiku-4-5"));
        assert_eq!(target.effort.as_deref(), Some("low"));
    }

    #[test]
    fn class_entry_falls_to_provider_default_when_model_unlisted() {
        let classes = classes(&[(
            TaskClass::Demanding,
            entry(ProviderKind::Claude, Some("claude-opus-6"), Some("max")),
        )]);
        let candidates = [candidate(ProviderKind::Claude, &["claude-sonnet-5"])];
        let (target, note) = resolve_entry(
            classes.get(&TaskClass::Demanding).unwrap(),
            &candidates,
            None,
        );
        assert_eq!(note, Some("model-ineligible"));
        assert_eq!(target.provider, ProviderKind::Claude);
        assert_eq!(target.model, None);
        assert_eq!(target.effort, None);
    }

    #[test]
    fn class_entry_degrades_to_default_route_without_the_provider() {
        let classes = classes(&[(
            TaskClass::General,
            entry(ProviderKind::Claude, Some("claude-sonnet-5"), None),
        )]);
        let candidates = [candidate(ProviderKind::Codex, &["gpt-5.5"])];
        let last_used = RouteTarget {
            provider: ProviderKind::Codex,
            model: Some("gpt-5.5".into()),
            effort: None,
        };
        let (target, note) = resolve_entry(
            classes.get(&TaskClass::General).unwrap(),
            &candidates,
            Some(&last_used),
        );
        assert_eq!(note, Some("provider-ineligible"));
        assert_eq!(target.provider, ProviderKind::Codex);
        assert_eq!(target.model.as_deref(), Some("gpt-5.5"));
    }

    #[test]
    fn provider_default_entry_keeps_its_effort() {
        let classes = classes(&[(
            TaskClass::General,
            entry(ProviderKind::Claude, None, Some("high")),
        )]);
        let candidates = [candidate(ProviderKind::Claude, &["claude-sonnet-5"])];
        let (target, note) =
            resolve_entry(classes.get(&TaskClass::General).unwrap(), &candidates, None);
        assert_eq!(note, None);
        assert_eq!(target.model, None);
        assert_eq!(target.effort.as_deref(), Some("high"));
    }
}
