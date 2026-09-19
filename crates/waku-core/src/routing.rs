//! New-session model routing: evaluate the first prompt into task family,
//! class, and planning answers, then resolve the routing policy against the
//! app's eligible candidate set to a concrete provider/model.
//!
//! The evaluator only names the kind of work — it never sees model ids and
//! its answers are untrusted input. Provider eligibility, confidence floors,
//! class floors, and the final target all come from ordinary deterministic
//! code in this module, and every decision records the reason chain that
//! produced it.

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::{Value, json};
use waku_protocol::eval::{EvalAnswer, EvalQuestion, EvalSettings, Evaluation};
use waku_protocol::model::ProviderKind;
use waku_protocol::routing::{RouteCandidate, RouteDecision, RouteTarget, TaskClass, TaskFamily};

use crate::eval::EvalDecisionRecord;
use crate::route_policy::{DefaultRoute, PolicyTarget, RoutePolicy, Tier};

/// One routing pass: the decision for the wire plus the log record that
/// explains it.
pub struct RouteRun {
    pub decision: RouteDecision,
    pub record: EvalDecisionRecord,
}

const FAMILY_INSTRUCTIONS: &str = "Classify the requested deliverable, not the topic or domain. \
Code or diffs the user wants produced or changed is code-generation; diagnosing a failure or \
critiquing existing code is debugging-review. New prose is writing, while revising text the user \
supplied is editing-rewriting. A factual answer or explanation is information-seeking; procedural \
steps are how-to-advice; tutoring requires the user to explicitly want teaching interaction. \
Numbers and structured data are data-math. Plans, options, and tradeoffs before acting are \
planning-ideation. Tasks that must read live state or change external systems through tools are \
agentic-tool-use — a task tools could merely help with is not. Media artifacts are creative-media. \
Everything else, including opinions and open conversation, is conversational-other.";

const CLASS_INSTRUCTIONS: &str = "How much model does this task deserve? routine is mechanical, \
low-risk, or single-step work where a fast cheap model suffices. demanding is subtle, high-stakes, \
or long-horizon work where mistakes are costly. general is the unmarked middle — pick it when the \
task is ordinary or the choice is unclear.";

const PLANNING_INSTRUCTIONS: &str = "Does completing this task well require a plan, design step, or \
multi-step decomposition before acting?";

/// Route one new-session submission. Never fails hard: every degraded path
/// still returns a usable target with the reason recorded.
pub fn route_task(
    eval_settings: Option<&EvalSettings>,
    policy: &RoutePolicy,
    prompt: &str,
    project: Option<&str>,
    candidates: &[RouteCandidate],
    last_used: Option<&RouteTarget>,
) -> RouteRun {
    let started = Instant::now();
    let mut reasons: Vec<&'static str> = Vec::new();
    let mut record = EvalDecisionRecord::empty("route");
    record.policy_hash = Some(policy.hash.clone());

    if candidates.is_empty() {
        reasons.push("no-candidates");
        return finish(
            record,
            last_used.cloned().unwrap_or_else(|| RouteTarget {
                provider: ProviderKind::default(),
                model: None,
            }),
            reasons,
            policy,
            started,
            eval_settings.map(|settings| settings.backend),
        );
    }

    let Some(eval_settings) = eval_settings else {
        reasons.push("eval-unconfigured");
        return finish(
            record,
            default_target(policy, candidates, last_used),
            reasons,
            policy,
            started,
            None,
        );
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
            reasons.push("eval-failed");
            return finish(
                record,
                default_target(policy, candidates, last_used),
                reasons,
                policy,
                started,
                Some(eval_settings.backend),
            );
        }
    };
    record.latency_ms = Some(evaluation.latency_ms);
    record.model = Some(evaluation.model.clone());
    record.answers = Some(evaluation.answers.clone());

    let family_answer = choice_answer(&evaluation, "family");
    let class_answer = choice_answer(&evaluation, "class");
    let planning_answer = noul_answer(&evaluation, "planning");

    let Some((class_id, class_confidence)) = class_answer else {
        reasons.push("eval-missing-answer");
        return finish(
            record,
            default_target(policy, candidates, last_used),
            reasons,
            policy,
            started,
            Some(eval_settings.backend),
        );
    };
    let Some(class) = parse_class_answer(&class_id) else {
        reasons.push("eval-missing-answer");
        return finish(
            record,
            default_target(policy, candidates, last_used),
            reasons,
            policy,
            started,
            Some(eval_settings.backend),
        );
    };
    let family_confidence = family_answer
        .as_ref()
        .and_then(|(_, confidence)| *confidence);
    let classified_family = family_answer.and_then(|(choice, _)| parse_family_answer(&choice));
    let family = classified_family
        .filter(|_| family_confidence.unwrap_or(1.0) >= policy.min_family_confidence);
    if classified_family.is_some() && family.is_none() {
        reasons.push("family-below-floor");
    }

    let needs_planning = planning_answer.map(|noul| noul >= 0.5);
    let decision_fields = DecisionFields {
        family,
        class: Some(class),
        needs_planning,
        family_confidence,
        class_confidence,
    };

    if class_confidence.unwrap_or(1.0) < policy.min_class_confidence {
        reasons.push("low-class-confidence");
        let mut run = finish(
            record,
            default_target(policy, candidates, last_used),
            reasons,
            policy,
            started,
            Some(eval_settings.backend),
        );
        apply_fields(&mut run.decision, &decision_fields);
        run.record.complete(&run.decision);
        return run;
    }

    let mut effective_class = class;
    if needs_planning == Some(true) && policy.planning_boost {
        effective_class = bump_class(effective_class);
        reasons.push("planning-boost");
    }
    if let Some(min_class) = family
        .and_then(|family| policy.family_overrides.get(&family))
        .and_then(|family_override| family_override.min_class)
    {
        if effective_class < min_class {
            effective_class = min_class;
            reasons.push("family-floor");
        }
    }

    let target = family
        .and_then(|family| policy.family_overrides.get(&family))
        .and_then(|family_override| family_override.classes.get(&effective_class))
        .or_else(|| policy.classes.get(&effective_class))
        .expect("a validated policy covers every class");

    reasons.push("policy");
    let (route_target, note) = resolve_target(target, policy, candidates, last_used);
    if let Some(note) = note {
        reasons.push(note);
    }
    let mut run = finish(
        record,
        route_target,
        reasons,
        policy,
        started,
        Some(eval_settings.backend),
    );
    apply_fields(&mut run.decision, &decision_fields);
    run.record.complete(&run.decision);
    run
}

/// The fields the classifier contributed, applied onto whichever target the
/// resolution produced — including fallback targets.
struct DecisionFields {
    family: Option<TaskFamily>,
    class: Option<TaskClass>,
    needs_planning: Option<bool>,
    family_confidence: Option<f64>,
    class_confidence: Option<f64>,
}

fn apply_fields(decision: &mut RouteDecision, fields: &DecisionFields) {
    decision.family = fields.family;
    decision.class = fields.class;
    decision.needs_planning = fields.needs_planning;
    decision.family_confidence = fields.family_confidence;
    decision.class_confidence = fields.class_confidence;
}

fn finish(
    mut record: EvalDecisionRecord,
    target: RouteTarget,
    reasons: Vec<&'static str>,
    policy: &RoutePolicy,
    started: Instant,
    backend: Option<waku_protocol::eval::EvalBackend>,
) -> RouteRun {
    let decision = RouteDecision {
        target,
        family: None,
        class: None,
        needs_planning: None,
        family_confidence: None,
        class_confidence: None,
        reason: reasons.join("+"),
        policy_hash: policy.hash.clone(),
        backend,
        eval_latency_ms: Some(started.elapsed().as_millis() as u64),
    };
    record.complete(&decision);
    RouteRun { decision, record }
}

/// The three questions every route asks, shared so the decision log can
/// reconstruct exactly what the classifier saw.
pub fn routing_questions() -> BTreeMap<String, EvalQuestion> {
    BTreeMap::from([
        (
            "family".to_owned(),
            EvalQuestion::Choice {
                instructions: FAMILY_INSTRUCTIONS.to_owned(),
                criteria: TaskFamily::ALL
                    .iter()
                    .map(|family| {
                        (
                            family.id().to_owned(),
                            Some(family.description().to_owned()),
                        )
                    })
                    .collect(),
            },
        ),
        (
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
        ),
        (
            "planning".to_owned(),
            EvalQuestion::Noul {
                instructions: PLANNING_INSTRUCTIONS.to_owned(),
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

fn noul_answer(evaluation: &Evaluation, key: &str) -> Option<f64> {
    match evaluation.answers.get(key) {
        Some(EvalAnswer::Noul { noul }) => Some(*noul),
        _ => None,
    }
}

fn parse_family_answer(choice: &str) -> Option<TaskFamily> {
    TaskFamily::ALL
        .iter()
        .find(|family| family.id() == choice)
        .copied()
}

fn parse_class_answer(choice: &str) -> Option<TaskClass> {
    serde_json::from_value(Value::String(choice.to_owned())).ok()
}

fn bump_class(class: TaskClass) -> TaskClass {
    match class {
        TaskClass::Routine => TaskClass::General,
        TaskClass::General => TaskClass::Demanding,
        TaskClass::Demanding => TaskClass::Demanding,
    }
}

fn find_candidate(
    candidates: &[RouteCandidate],
    provider: ProviderKind,
) -> Option<&RouteCandidate> {
    candidates
        .iter()
        .find(|candidate| candidate.provider == provider)
}

/// A concrete (provider, model) pair is usable when the provider is a
/// candidate and the model appears in its live catalog — or the catalog is
/// unknown (`models` empty), in which case an explicit model is accepted.
fn is_eligible(candidate: &RouteCandidate, model: Option<&str>) -> bool {
    match model {
        None => true,
        Some(model) => candidate.models.is_empty() || candidate.models.iter().any(|m| m == model),
    }
}

/// Providers in preference order: policy-preferred first, then the remaining
/// candidates in request order.
fn ordered_candidates<'a>(
    policy: &RoutePolicy,
    candidates: &'a [RouteCandidate],
) -> Vec<&'a RouteCandidate> {
    let mut ordered: Vec<&RouteCandidate> = policy
        .preferred_providers
        .iter()
        .filter_map(|provider| find_candidate(candidates, *provider))
        .collect();
    for candidate in candidates {
        if !ordered.iter().any(|c| c.provider == candidate.provider) {
            ordered.push(candidate);
        }
    }
    ordered
}

/// The tier's model id inside one provider's tier table, validated against
/// the live catalog — `None` when the tier is unmapped or the catalog lacks
/// it, leaving the provider's own default.
fn tier_model(
    policy: &RoutePolicy,
    candidate: &RouteCandidate,
    tier: Tier,
) -> Option<String> {
    policy
        .tiers
        .get(&candidate.provider)
        .and_then(|table| table.get(&tier))
        .filter(|model| is_eligible(candidate, Some(model.as_str())))
        .cloned()
}

/// Resolve a policy target to an eligible concrete route, returning the
/// target plus an optional reason note for the chain. A tier target takes
/// the first eligible provider in preference order — a session tier stays
/// inside the session's own provider, degrading to the global walk only
/// when that provider is not a candidate — and the model drops to that
/// provider's default when the tier is unmapped or the catalog lacks it. A
/// concrete target degrades to the same provider's default before the
/// policy's default route is consulted.
fn resolve_target(
    target: &PolicyTarget,
    policy: &RoutePolicy,
    candidates: &[RouteCandidate],
    last_used: Option<&RouteTarget>,
) -> (RouteTarget, Option<&'static str>) {
    match target {
        PolicyTarget::Concrete { provider, model } => match find_candidate(candidates, *provider) {
            Some(candidate) if is_eligible(candidate, model.as_deref()) => (
                RouteTarget {
                    provider: *provider,
                    model: model.clone(),
                },
                None,
            ),
            Some(_) => (
                RouteTarget {
                    provider: *provider,
                    model: None,
                },
                Some("model-ineligible"),
            ),
            None => (
                default_target(policy, candidates, None),
                Some("provider-ineligible"),
            ),
        },
        PolicyTarget::SessionTier(tier) => {
            let session_candidate = last_used.and_then(|last| {
                find_candidate(candidates, last.provider)
                    .filter(|candidate| candidate.provider == last.provider)
            });
            match session_candidate {
                Some(candidate) => {
                    let model = tier_model(policy, candidate, *tier);
                    let fell_to_default = model.is_none();
                    (
                        RouteTarget {
                            provider: candidate.provider,
                            model,
                        },
                        fell_to_default.then_some("tier-fell-to-default"),
                    )
                }
                // No usable session provider — degrade to the global walk.
                None => {
                    let (target, note) =
                        resolve_target(&PolicyTarget::Tier(*tier), policy, candidates, last_used);
                    (target, note.or(Some("session-ineligible")))
                }
            }
        }
        PolicyTarget::Tier(tier) => {
            let Some(candidate) = ordered_candidates(policy, candidates).first().copied() else {
                return (
                    RouteTarget {
                        provider: ProviderKind::default(),
                        model: None,
                    },
                    Some("no-candidates"),
                );
            };
            let model = tier_model(policy, candidate, *tier);
            let fell_to_default = model.is_none();
            (
                RouteTarget {
                    provider: candidate.provider,
                    model,
                },
                fell_to_default.then_some("tier-fell-to-default"),
            )
        }
    }
}

/// The policy's configured default route: `last_used` (or its provider's
/// default) when the last target is still eligible, a configured target when
/// the user set one, otherwise the first eligible provider's default.
fn default_target(
    policy: &RoutePolicy,
    candidates: &[RouteCandidate],
    last_used: Option<&RouteTarget>,
) -> RouteTarget {
    match &policy.default {
        DefaultRoute::Target(target) => resolve_target(target, policy, candidates, last_used).0,
        DefaultRoute::LastUsed => {
            if let Some(last) = last_used {
                if let Some(candidate) = find_candidate(candidates, last.provider) {
                    let model = last
                        .model
                        .as_deref()
                        .filter(|model| is_eligible(candidate, Some(model)))
                        .map(str::to_owned);
                    return RouteTarget {
                        provider: last.provider,
                        model,
                    };
                }
            }
            ordered_candidates(policy, candidates)
                .first()
                .map(|candidate| RouteTarget {
                    provider: candidate.provider,
                    model: None,
                })
                .unwrap_or_else(|| RouteTarget {
                    provider: ProviderKind::default(),
                    model: None,
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route_policy::shipped_default_policy;

    fn candidate(provider: ProviderKind, models: &[&str]) -> RouteCandidate {
        RouteCandidate {
            provider,
            models: models.iter().map(|model| model.to_string()).collect(),
        }
    }

    #[test]
    fn unconfigured_eval_falls_back_to_last_used() {
        let policy = shipped_default_policy();
        let candidates = [
            candidate(ProviderKind::Claude, &["claude-sonnet-5"]),
            candidate(ProviderKind::Codex, &["gpt-5.5"]),
        ];
        let last_used = RouteTarget {
            provider: ProviderKind::Codex,
            model: Some("gpt-5.5".into()),
        };
        let run = route_task(
            None,
            &policy,
            "fix the typo",
            None,
            &candidates,
            Some(&last_used),
        );
        assert_eq!(run.decision.target.provider, ProviderKind::Codex);
        assert_eq!(run.decision.target.model.as_deref(), Some("gpt-5.5"));
        assert!(run.decision.reason.contains("eval-unconfigured"));
    }

    #[test]
    fn unconfigured_eval_without_last_used_picks_preferred_provider() {
        let policy = shipped_default_policy();
        // Codex sorts before Claude in request order, but the shipped policy
        // prefers Claude.
        let candidates = [
            candidate(ProviderKind::Codex, &["gpt-5.5"]),
            candidate(ProviderKind::Claude, &["claude-sonnet-5"]),
        ];
        let run = route_task(None, &policy, "fix the typo", None, &candidates, None);
        assert_eq!(run.decision.target.provider, ProviderKind::Claude);
    }

    #[test]
    fn ineligible_last_used_model_drops_to_provider_default() {
        let policy = shipped_default_policy();
        let candidates = [candidate(ProviderKind::Codex, &["gpt-5.5"])];
        let last_used = RouteTarget {
            provider: ProviderKind::Codex,
            model: Some("gpt-4-removed".into()),
        };
        let run = route_task(None, &policy, "hello", None, &candidates, Some(&last_used));
        assert_eq!(run.decision.target.provider, ProviderKind::Codex);
        assert_eq!(run.decision.target.model, None);
    }

    #[test]
    fn empty_candidates_still_returns_a_target() {
        let policy = shipped_default_policy();
        let run = route_task(None, &policy, "hello", None, &[], None);
        assert_eq!(run.decision.reason, "no-candidates");
        // The decision is still a usable provider default.
        assert_eq!(run.decision.target.provider, ProviderKind::default());
        assert!(run.record.resolved_provider.is_some());
    }

    #[test]
    fn tier_target_resolves_through_provider_table() {
        let policy = shipped_default_policy();
        let candidates = [
            candidate(
                ProviderKind::Claude,
                &["claude-haiku-4-5", "claude-sonnet-5", "claude-opus-5"],
            ),
            candidate(ProviderKind::Codex, &["gpt-5.6-luna", "gpt-5.6-sol"]),
        ];
        // Claude is the first preferred provider, so tier:fast maps to haiku.
        let (target, note) = resolve_target(
            &PolicyTarget::Tier(crate::route_policy::Tier::Fast),
            &policy,
            &candidates,
            None,
        );
        assert_eq!(target.provider, ProviderKind::Claude);
        assert_eq!(target.model.as_deref(), Some("claude-haiku-4-5"));
        assert_eq!(note, None);
    }

    #[test]
    fn tier_target_falls_to_provider_default_when_unmapped() {
        let policy = shipped_default_policy();
        // Cursor has no tiers entry in the shipped policy and is preferred
        // over an unlisted provider.
        let candidates = [
            candidate(ProviderKind::Cursor, &["auto"]),
            candidate(ProviderKind::DeepSeek, &["deepseek-chat"]),
        ];
        let (target, note) = resolve_target(
            &PolicyTarget::Tier(crate::route_policy::Tier::Fast),
            &policy,
            &candidates,
            None,
        );
        assert_eq!(target.provider, ProviderKind::Cursor);
        assert_eq!(target.model, None);
        assert_eq!(note, Some("tier-fell-to-default"));
    }

    #[test]
    fn concrete_target_with_ineligible_provider_uses_default_route() {
        let policy = shipped_default_policy();
        let candidates = [candidate(ProviderKind::Claude, &["claude-sonnet-5"])];
        let (target, note) = resolve_target(
            &PolicyTarget::Concrete {
                provider: ProviderKind::Kimi,
                model: Some("k2".into()),
            },
            &policy,
            &candidates,
            None,
        );
        assert_eq!(target.provider, ProviderKind::Claude);
        assert_eq!(note, Some("provider-ineligible"));
    }

    #[test]
    fn session_tier_resolves_inside_the_session_provider() {
        let policy = shipped_default_policy();
        // Codex is the session provider — the tier stays there even though
        // the preference order would pick Claude's catalog first.
        let candidates = [
            candidate(
                ProviderKind::Claude,
                &["claude-haiku-4-5", "claude-sonnet-5"],
            ),
            candidate(
                ProviderKind::Codex,
                &["gpt-5.6-luna", "gpt-5.6-sol"],
            ),
        ];
        let last_used = RouteTarget {
            provider: ProviderKind::Codex,
            model: Some("gpt-5.6-sol".into()),
        };
        let (target, note) = resolve_target(
            &PolicyTarget::SessionTier(crate::route_policy::Tier::Fast),
            &policy,
            &candidates,
            Some(&last_used),
        );
        assert_eq!(target.provider, ProviderKind::Codex);
        assert_eq!(target.model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(note, None);
    }

    #[test]
    fn session_tier_degrades_to_preference_order_without_a_session_provider() {
        let policy = shipped_default_policy();
        let candidates = [
            candidate(
                ProviderKind::Claude,
                &["claude-haiku-4-5"],
            ),
            candidate(ProviderKind::Kimi, &["k2"]),
        ];
        // The draft's provider is not a candidate — the tier falls back to
        // the same preference-order walk a global tier takes.
        let last_used = RouteTarget {
            provider: ProviderKind::Amp,
            model: None,
        };
        let (target, note) = resolve_target(
            &PolicyTarget::SessionTier(crate::route_policy::Tier::Fast),
            &policy,
            &candidates,
            Some(&last_used),
        );
        assert_eq!(target.provider, ProviderKind::Claude);
        assert_eq!(target.model.as_deref(), Some("claude-haiku-4-5"));
        assert_eq!(note, Some("session-ineligible"));
    }
}
