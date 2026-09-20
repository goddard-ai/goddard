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

const CLASS_INSTRUCTIONS: &str = "How much model does this task deserve? routine is mechanical, \
low-risk, or single-step work where a fast cheap model suffices. demanding is subtle, high-stakes, \
or long-horizon work where mistakes are costly. general is the unmarked middle — pick it when the \
task is ordinary or the choice is unclear.";

/// The class answer must clear this confidence before it routes; below it
/// the session keeps the default route.
const MIN_CLASS_CONFIDENCE: f64 = 0.55;

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
    let started = Instant::now();
    let mut record = EvalDecisionRecord::empty("route");
    // What the classifier contributed, applied onto whichever target the
    // resolution produced — including fallback targets.
    let mut class_answered: Option<TaskClass> = None;
    let mut class_confidence: Option<f64> = None;

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
        record.backend = Some(eval_settings.backend);

        let state = json!({ "task": prompt, "project": project });
        let questions = routing_questions();
        record.state = Some(state.clone());
        record.questions = Some(questions.clone());

        let evaluation = match crate::eval::evaluate(eval_settings, &state, &questions) {
            Ok(evaluation) => evaluation,
            Err(error) => {
                record.error = Some(error.to_string());
                break 'resolve default(vec!["eval-failed"]);
            }
        };
        record.latency_ms = Some(evaluation.latency_ms);
        record.model = Some(evaluation.model.clone());
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

        match classes.get(&class) {
            Some(entry) => {
                let (target, note) = resolve_entry(entry, candidates, last_used);
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
        class_confidence,
        reason: reasons.join("+"),
        backend: eval_settings.map(|settings| settings.backend),
        eval_latency_ms: Some(started.elapsed().as_millis() as u64),
    };
    record.complete(&decision);
    RouteRun { decision, record }
}

/// The question every route asks, shared so the decision log can reconstruct
/// exactly what the classifier saw.
pub fn routing_questions() -> BTreeMap<String, EvalQuestion> {
    BTreeMap::from([(
        "class".to_owned(),
        EvalQuestion::Choice {
            instructions: CLASS_INSTRUCTIONS.to_owned(),
            criteria: BTreeMap::from([
                (
                    "routine".to_owned(),
                    Some("mechanical, low-risk, or single-step work".to_owned()),
                ),
                (
                    "general".to_owned(),
                    Some("ordinary work; the default".to_owned()),
                ),
                (
                    "demanding".to_owned(),
                    Some("subtle, high-stakes, or long-horizon work".to_owned()),
                ),
            ]),
        },
    )])
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

    fn entry(provider: ProviderKind, model: Option<&str>, effort: Option<&str>) -> RouteClassTarget {
        RouteClassTarget {
            provider,
            model: model.map(str::to_owned),
            effort: effort.map(str::to_owned),
        }
    }

    fn classes(pairs: &[(TaskClass, RouteClassTarget)]) -> RouteClassMap {
        pairs.iter().cloned().collect()
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
        let run = route_task(
            None,
            &classes,
            "hello",
            None,
            &candidates,
            Some(&last_used),
        );
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
        let (target, note) = resolve_entry(
            classes.get(&TaskClass::Routine).unwrap(),
            &candidates,
            None,
        );
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
        let (target, note) = resolve_entry(
            classes.get(&TaskClass::General).unwrap(),
            &candidates,
            None,
        );
        assert_eq!(note, None);
        assert_eq!(target.model, None);
        assert_eq!(target.effort.as_deref(), Some("high"));
    }
}
