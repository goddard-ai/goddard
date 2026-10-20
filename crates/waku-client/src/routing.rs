//! Per-turn reasoning effort and conservative model handoffs for Auto tasks.

use std::collections::BTreeMap;

use waku_protocol::eval::{EvalAnswer, EvalQuestion, Evaluation};
use waku_protocol::model::{ProviderKind, ProviderModel, ProviderModelOption};
use waku_protocol::routing::{RouteClassMap, RouteDecision, RouteTarget, TaskClass};

const TURN_EFFORT_INSTRUCTIONS: &str = "Which reasoning effort should the current model use for \
nextRequest, given what previousTurn establishes and what remains unresolved? Judge whether more \
deliberate thinking would materially improve this turn's correctness or approach. Use medium or \
normal effort when offered as the baseline for ordinary analysis and execution of a settled \
approach; otherwise use defaultEffort or the closest supported ordinary level. Use lower effort \
for mechanical work needing little judgment. Increase effort for competing plausible explanations, \
subtle interacting constraints, or several dependent deductions where deeper analysis is likely \
to help. Reserve xhigh or max for exceptionally demanding reasoning where a smaller increase is \
unlikely to suffice. Duration, scope, file count, high stakes, missing information, and labels like \
planning or implementation do not by themselves justify increased effort. Implementation may still \
need deeper reasoning if important decisions remain unresolved. Judge the upcoming work rather \
than inheriting the task's initial difficulty or currentEffort. Select only an offered effort; \
this question does not select a different model.";

fn turn_effort_description(option: &ProviderModelOption) -> Option<String> {
    let description = match option.id.as_str() {
        "none" => "No extended reasoning; direct mechanical execution.",
        "minimal" => "Minimal reasoning for obvious, mechanical work.",
        "low" => "Light reasoning for straightforward work with little judgment.",
        "medium" | "normal" => {
            "Normal reasoning for ordinary analysis and execution of a settled approach."
        }
        "high" => {
            "More deliberate analysis that materially helps resolve an important reasoning bottleneck."
        }
        "xhigh" => {
            "Exceptionally thorough reasoning for a bottleneck that high effort is unlikely to resolve reliably."
        }
        "max" => {
            "Maximum reasoning, reserved for exceptional bottlenecks where smaller increases are unlikely to suffice."
        }
        _ => return option.description.clone(),
    };
    Some(description.to_owned())
}

/// Judge effort on the current model independently of model-tier selection.
pub fn turn_effort_question(efforts: &[ProviderModelOption]) -> EvalQuestion {
    EvalQuestion::Choice {
        instructions: TURN_EFFORT_INSTRUCTIONS.to_owned(),
        criteria: efforts
            .iter()
            .map(|effort| (effort.id.clone(), turn_effort_description(effort)))
            .collect(),
    }
}

/// An approved class-map target and the map that supplied it.
#[derive(Clone, Debug, PartialEq)]
pub struct HandoffTarget {
    pub target: RouteTarget,
    pub provider_map: bool,
}

/// The two distinct same-provider models an Auto task may hand off between.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelHandoff {
    pub medium: HandoffTarget,
    pub hard: HandoffTarget,
}

impl ModelHandoff {
    /// Return an approved Medium or Hard target; Easy is never offered.
    pub fn target(&self, class: TaskClass) -> Option<&HandoffTarget> {
        match class {
            TaskClass::General => Some(&self.medium),
            TaskClass::Demanding => Some(&self.hard),
            TaskClass::Routine => None,
        }
    }

    /// Identify which approved model currently owns the work.
    pub fn current_class(&self, model: &str) -> Option<TaskClass> {
        if self.hard.target.model.as_deref() == Some(model) {
            Some(TaskClass::Demanding)
        } else if self.medium.target.model.as_deref() == Some(model) {
            Some(TaskClass::General)
        } else {
            None
        }
    }
}

/// Skip the judgment unless it can change a model inside this provider.
/// Legacy planning overrides are eligible too, so their Hard starts can
/// return to the workhorse. Manual choices clear the routing decision.
pub fn model_handoff_targets(
    enabled: bool,
    decision: Option<&RouteDecision>,
    provider: ProviderKind,
    current_model: &str,
    provider_map: Option<&RouteClassMap>,
    classes: &RouteClassMap,
    catalog: &[ProviderModel],
) -> Option<ModelHandoff> {
    let decision = decision.filter(|_| enabled)?;
    let legacy_hard = decision.applied_class.is_none()
        && decision.reason.split('+').next() == Some("class-map")
        && (decision.class == Some(TaskClass::Demanding)
            || decision
                .reason
                .split('+')
                .any(|leg| leg == "phase-planning"));
    if decision.applied_class != Some(TaskClass::Demanding) && !legacy_hard {
        return None;
    }
    let target = |class| {
        let (entry, provider_map) = provider_map
            .and_then(|map| map.get(&class))
            .map(|entry| (entry, true))
            .or_else(|| {
                classes
                    .get(&class)
                    .filter(|entry| entry.provider == provider)
                    .map(|entry| (entry, false))
            })?;
        let model = entry.model.as_ref()?;
        if !catalog.iter().any(|candidate| &candidate.id == model) {
            return None;
        }
        Some(HandoffTarget {
            target: RouteTarget {
                provider,
                model: Some(model.clone()),
                effort: entry.effort.clone(),
            },
            provider_map,
        })
    };
    let handoff = ModelHandoff {
        medium: target(TaskClass::General)?,
        hard: target(TaskClass::Demanding)?,
    };
    if handoff.medium.target.model == handoff.hard.target.model {
        return None;
    }
    handoff.current_class(current_model)?;
    Some(handoff)
}

/// Judge the next request against settled context, with the current tier as default.
pub fn model_handoff_questions() -> BTreeMap<String, EvalQuestion> {
    BTreeMap::from([(
        "remaining_work".into(),
        EvalQuestion::Choice {
            instructions: "Which model tier does the NEXT request need, given the previous \
                settled turn? Medium is the user's normal workhorse. Judge unresolved reasoning \
                in the upcoming work, not duration, scope, implementation volume, or phase labels. \
                Choose hard when deeper analysis is likely to materially improve correctness or \
                the approach beyond the workhorse: distinguishing competing plausible explanations, \
                reconciling subtle interacting constraints, or making several dependent deductions. \
                Missing information, unfamiliar code, and high stakes alone do not establish hard. \
                Choose medium when the important decisions are resolved and ordinary analysis, \
                implementation, verification, or explanation can reliably finish the work. A plan \
                or first code edit alone does not establish that those decisions are resolved; \
                implementation may still expose a reasoning bottleneck. Account for nextRequest \
                and what previousTurn establishes. Treat currentClass as the default when evidence \
                is unclear."
                .into(),
            criteria: BTreeMap::from([
                (
                    "medium".into(),
                    Some("the workhorse can reliably handle the remaining work".into()),
                ),
                (
                    "hard".into(),
                    Some("a remaining reasoning bottleneck makes deeper analysis materially valuable beyond the workhorse".into()),
                ),
            ]),
        },
    )])
}

/// A downshift needs stronger evidence than restoring Hard. Missing,
/// malformed, or ambiguous answers keep the model already in use.
pub fn model_handoff_class(evaluation: &Evaluation, current: TaskClass) -> Option<TaskClass> {
    let EvalAnswer::Choice {
        choice,
        confidence: Some(confidence),
        probabilities,
    } = evaluation.answers.get("remaining_work")?
    else {
        return None;
    };
    let medium = *probabilities.get("medium")?;
    let hard = *probabilities.get("hard")?;
    if probabilities.len() != 2
        || [*confidence, medium, hard]
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        || (medium + hard - 1.0).abs() > 0.01
    {
        return None;
    }
    match (current, choice.as_str()) {
        (TaskClass::Demanding, "medium") if *confidence >= 0.75 && medium >= 0.85 => {
            Some(TaskClass::General)
        }
        (TaskClass::General, "hard") if *confidence >= 0.7 && hard >= 0.75 => {
            Some(TaskClass::Demanding)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::routing::RouteClassTarget;

    fn decision(class: TaskClass) -> RouteDecision {
        RouteDecision {
            target: RouteTarget {
                provider: ProviderKind::Codex,
                model: Some("hard-model".into()),
                effort: None,
            },
            class: Some(class),
            applied_class: Some(class),
            class_confidence: Some(0.9),
            phased: false,
            reason: "class-map".into(),
            backend: None,
            eval_latency_ms: None,
        }
    }

    fn classes() -> RouteClassMap {
        [
            (TaskClass::General, "medium-model"),
            (TaskClass::Demanding, "hard-model"),
        ]
        .into_iter()
        .map(|(class, model)| {
            (
                class,
                RouteClassTarget {
                    provider: ProviderKind::Codex,
                    model: Some(model.into()),
                    effort: None,
                },
            )
        })
        .collect()
    }

    fn catalog() -> Vec<ProviderModel> {
        ["medium-model", "hard-model"]
            .into_iter()
            .map(|id| ProviderModel::new(id, id))
            .collect()
    }

    #[test]
    fn only_actionable_auto_handoffs_are_eligible() {
        let hard = decision(TaskClass::Demanding);
        let mut medium = decision(TaskClass::General);
        medium.phased = true; // An old planning verdict alone does not authorize a handoff.
        let mut fallback = hard.clone();
        fallback.applied_class = None;
        fallback.reason = "low-class-confidence".into();
        let classes = classes();
        let catalog = catalog();
        let eligible =
            |enabled, decision, current, classes: &RouteClassMap, catalog: &[ProviderModel]| {
                model_handoff_targets(
                    enabled,
                    decision,
                    ProviderKind::Codex,
                    current,
                    None,
                    classes,
                    catalog,
                )
            };
        assert!(eligible(true, Some(&hard), "hard-model", &classes, &catalog).is_some());
        assert!(eligible(true, Some(&hard), "medium-model", &classes, &catalog).is_some());
        assert!(eligible(false, Some(&hard), "hard-model", &classes, &catalog).is_none());
        assert!(eligible(true, None, "hard-model", &classes, &catalog).is_none());
        assert!(eligible(true, Some(&fallback), "hard-model", &classes, &catalog).is_none());
        assert!(eligible(true, Some(&medium), "medium-model", &classes, &catalog).is_none());
        assert!(eligible(true, Some(&hard), "other-model", &classes, &catalog).is_none());
        assert!(eligible(true, Some(&hard), "hard-model", &classes, &catalog[..1]).is_none());
        let mut same = classes.clone();
        same.get_mut(&TaskClass::General).unwrap().model = Some("hard-model".into());
        assert!(eligible(true, Some(&hard), "hard-model", &same, &catalog).is_none());
        let mut cross_provider = classes.clone();
        cross_provider
            .get_mut(&TaskClass::General)
            .unwrap()
            .provider = ProviderKind::Claude;
        assert!(eligible(true, Some(&hard), "hard-model", &cross_provider, &catalog).is_none());
        let mut missing = classes.clone();
        missing.remove(&TaskClass::General);
        assert!(eligible(true, Some(&hard), "hard-model", &missing, &catalog).is_none());
    }

    #[test]
    fn provider_map_is_authoritative_and_legacy_hard_starts_are_eligible() {
        let mut legacy = decision(TaskClass::General);
        legacy.applied_class = Some(TaskClass::Demanding);
        legacy.phased = true;
        let mut overrides = classes();
        overrides.get_mut(&TaskClass::General).unwrap().provider = ProviderKind::Claude;
        overrides.get_mut(&TaskClass::General).unwrap().effort = Some("high".into());
        let handoff = model_handoff_targets(
            true,
            Some(&legacy),
            ProviderKind::Codex,
            "hard-model",
            Some(&overrides),
            &RouteClassMap::new(),
            &catalog(),
        )
        .unwrap();
        assert_eq!(handoff.medium.target.provider, ProviderKind::Codex);
        assert_eq!(handoff.medium.target.effort.as_deref(), Some("high"));
        assert!(handoff.medium.provider_map);
    }

    fn answer(choice: &str, confidence: Option<f64>, medium: f64, hard: f64) -> Evaluation {
        Evaluation {
            model: "test".into(),
            answers: BTreeMap::from([(
                "remaining_work".into(),
                EvalAnswer::Choice {
                    choice: choice.into(),
                    confidence,
                    probabilities: BTreeMap::from([
                        ("medium".into(), medium),
                        ("hard".into(), hard),
                    ]),
                },
            )]),
            usage: Default::default(),
            latency_ms: 0,
            provider_metadata: None,
        }
    }

    #[test]
    fn model_moves_need_confident_asymmetric_evidence() {
        assert_eq!(
            model_handoff_class(&answer("medium", Some(0.9), 0.9, 0.1), TaskClass::Demanding),
            Some(TaskClass::General)
        );
        assert_eq!(
            model_handoff_class(&answer("hard", Some(0.8), 0.2, 0.8), TaskClass::General),
            Some(TaskClass::Demanding)
        );
        assert_eq!(
            model_handoff_class(&answer("medium", Some(0.9), 0.8, 0.2), TaskClass::Demanding),
            None
        );
        assert_eq!(
            model_handoff_class(&answer("hard", Some(0.9), 0.3, 0.7), TaskClass::General),
            None
        );
        assert_eq!(
            model_handoff_class(
                &answer("medium", Some(0.7), 0.95, 0.05),
                TaskClass::Demanding
            ),
            None
        );
        assert_eq!(
            model_handoff_class(&answer("medium", Some(0.9), 0.9, 0.1), TaskClass::General),
            None
        );
    }

    #[test]
    fn malformed_or_missing_answers_keep_the_current_model() {
        for evaluation in [
            answer("medium", None, 0.95, 0.05),
            answer("unknown", Some(0.9), 0.95, 0.05),
            answer("medium", Some(f64::NAN), 0.95, 0.05),
            answer("medium", Some(1.1), 0.95, 0.05),
            answer("medium", Some(0.9), 1.2, -0.2),
            answer("medium", Some(0.9), 0.95, f64::NAN),
            answer("medium", Some(0.9), 0.95, 0.95),
        ] {
            assert_eq!(model_handoff_class(&evaluation, TaskClass::Demanding), None);
        }
        let mut missing = answer("medium", Some(0.9), 0.95, 0.05);
        missing.answers.clear();
        assert_eq!(model_handoff_class(&missing, TaskClass::Demanding), None);
        let mut missing_probability = answer("medium", Some(0.9), 0.95, 0.05);
        if let Some(EvalAnswer::Choice { probabilities, .. }) =
            missing_probability.answers.get_mut("remaining_work")
        {
            probabilities.remove("hard");
        }
        assert_eq!(
            model_handoff_class(&missing_probability, TaskClass::Demanding),
            None
        );
    }
}
