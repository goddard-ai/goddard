//! Auto model routing: a draft whose model selection is Auto asks the
//! daemon's evaluation model to classify its first prompt, and the daemon
//! resolves that classification through the user's class map to a concrete
//! provider/model/effort. Subsequent turns can adjust reasoning effort and,
//! with adaptive routing enabled, hand Hard tasks to the workhorse model.
//! Judgments run before a prompt, using the previous settled turn and the
//! next request; streamed phase labels never select a model.
//!
//! This file is the app-side seam. [`RouteStartPlan`] snapshots everything
//! the resolved provider's start request needs while probes and settings are
//! on the UI thread; the blocking route call and provider spawn then run
//! inside `prepare_submission` like every other first-turn start. The
//! classifier only names the kind of work — provider eligibility, the
//! binary, model traits, and the final start request are ordinary app code
//! here, the same as an unpicked provider default.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::anyhow;
use serde_json::{Value, json};
use uuid::Uuid;
use waku_client::routing::{
    ModelHandoff, model_handoff_class, model_handoff_questions, model_handoff_targets,
};
use waku_protocol::eval::{EvalAnswer, EvalQuestion};
use waku_protocol::model::{ProviderKind, ProviderResumeCursor, RuntimeMode};
use waku_protocol::routing::{RouteCandidate, RouteDecision, RouteTarget, TaskClass};

use super::runtime::start_driver;
use super::*;

/// How a first turn's provider start is chosen: a request built at accept
/// time, or a routing plan that resolves through the eval backend first.
pub(super) enum SessionStartPlan {
    Direct(anyhow::Result<DriverStartRequest>),
    Routed(Box<RouteStartPlan>),
}

/// Everything needed to route a draft's first prompt and start the resolved
/// provider, captured while the session's own state is still on the UI
/// thread. Provider/model-dependent request fields resolve after the daemon
/// answers; everything else is carried verbatim.
pub(super) struct RouteStartPlan {
    session_id: Uuid,
    /// The user-visible prompt text the classifier sees — never
    /// provider-resolved syntax.
    prompt: String,
    project: Option<String>,
    candidates: Vec<RouteCandidate>,
    last_used: Option<RouteTarget>,
    /// The session's daemon: serves the RouteTask RPC and the provider start.
    daemon: waku_client::DaemonSupervisor,
    event_wake: smol::channel::Sender<()>,
    /// Per-candidate CLI path — the local probe's path, or the remote host's
    /// override/command fallback, mirroring `provider_binary_for_session`.
    binaries: BTreeMap<ProviderKind, Option<PathBuf>>,
    /// Per-candidate default model id for routes that land on a provider
    /// default (`model: None`).
    preferred_models: BTreeMap<ProviderKind, Option<String>>,
    /// The draft's resolved preset — only reused when the route keeps the
    /// session on the provider the preset belongs to.
    agent_preset: Option<String>,
    deepseek_preferred_preset: Option<String>,
    provider_cursor: Option<ProviderResumeCursor>,
    /// The session's own history should be readable by its agent (a switch
    /// or a side chat) even with the cross-task agent tools off.
    read_own_transcript: bool,
    mode: RuntimeMode,
    computer_use_enabled: bool,
    /// The user's remembered (effort, tier, window) triples, so the routed
    /// model starts with the traits last picked for it.
    remembered_traits: Vec<waku_client::persistence::RememberedModelTraits>,
    /// The draft's own start request — the fallback when the route RPC
    /// itself fails. The daemon's route never fails hard; transport can.
    fallback: anyhow::Result<DriverStartRequest>,
}

impl RouteStartPlan {
    /// Route the prompt, then start the resolved provider — the blocking
    /// half of an Auto submission, run on the background executor inside
    /// `prepare_submission`. The decision rides back for the session record;
    /// `None` means the route call failed and the draft's own provider
    /// started instead.
    pub(super) fn route_and_start(
        self,
        cwd: PathBuf,
        stage: &StageReporter,
    ) -> anyhow::Result<(Option<RouteDecision>, PreparedDriver)> {
        SubmissionStage::Routing.report(stage);
        let routed = self.route().map(|decision| {
            self.start_request(&decision.target)
                .map(|request| (decision, request))
        });
        let (decision, request) = match routed {
            Ok(Ok((decision, request))) => (Some(decision), request),
            // A route call that cannot answer, or a resolved provider with no
            // binary, still owes the user a running session — the draft's own
            // provider stands in, the way an unrouted submission would start.
            Ok(Err(_)) | Err(_) => (None, self.fallback?),
        };
        SubmissionStage::Starting.report(stage);
        start_driver(request, cwd).map(|driver| (decision, driver))
    }

    fn route(&self) -> anyhow::Result<RouteDecision> {
        match self.daemon.client().request(
            Uuid::nil(),
            self.session_id,
            waku_client::Command::RouteTask {
                prompt: self.prompt.clone(),
                project: self.project.clone(),
                candidates: self.candidates.clone(),
                last_used: self.last_used.clone(),
            },
        )? {
            waku_client::ResponsePayload::RouteDecision { decision } => Ok(decision),
            _ => Err(anyhow!("the daemon returned an invalid route response")),
        }
    }

    /// The resolved target's start request — `driver_start_request_for_session`
    /// with provider, model, and traits substituted for the route's answer.
    /// `options.cwd` is a placeholder; `start_driver` fills in the
    /// materialized workspace path.
    fn start_request(&self, target: &RouteTarget) -> anyhow::Result<DriverStartRequest> {
        let provider = target.provider;
        let binary = self
            .binaries
            .get(&provider)
            .cloned()
            .flatten()
            .ok_or_else(|| {
                anyhow!(tr!(
                    "errors.provider_not_found",
                    provider = provider.display_name()
                ))
            })?;
        let model = target
            .model
            .clone()
            .or_else(|| self.preferred_models.get(&provider).cloned().flatten());
        let (remembered_effort, service_tier, context_window) = model
            .as_deref()
            .map(|model| {
                waku_client::persistence::remembered_model_traits_for(
                    &self.remembered_traits,
                    provider,
                    model,
                )
            })
            .unwrap_or_default();
        // The route's own effort wins; the remembered triple is the fallback
        // for targets — last-used, provider defaults — that name none.
        let reasoning_effort = target.effort.clone().or(remembered_effort);
        // A preset belongs to its provider: a route that stays keeps it, a
        // route that moved providers drops it — the same rule `choose_model`
        // applies on a provider switch. DeepSeek alone has presets, so only
        // it can gain one.
        let agent_preset = if provider == ProviderKind::DeepSeek {
            self.agent_preset
                .clone()
                .or_else(|| self.deepseek_preferred_preset.clone())
        } else {
            None
        };
        Ok(DriverStartRequest {
            session_id: self.session_id,
            provider,
            options: DriverStartOptions {
                binary,
                cwd: PathBuf::new(),
                mode: self.mode,
                model,
                reasoning_effort,
                service_tier,
                context_window,
                agent_preset,
                computer_use_enabled: self.computer_use_enabled,
                read_own_transcript: self.read_own_transcript,
                provider_cursor: self.provider_cursor.clone(),
            },
            event_wake: self.event_wake.clone(),
            daemon: self.daemon.clone(),
        })
    }
}

/// Snapshot a follow-up prompt's effort and optional model-handoff judgment
/// on the UI thread. The daemon call runs inside background preparation.
pub(super) struct TurnRoutePlan {
    session_id: Uuid,
    prompt: String,
    model: String,
    efforts: Vec<String>,
    current_effort: Option<String>,
    owner: RouteDecision,
    original_model: Option<String>,
    original_effort: Option<String>,
    original_service_tier: Option<String>,
    original_context_window: Option<String>,
    handoff: Option<ModelHandoff>,
    previous_turn: Option<Value>,
    daemon: waku_client::DaemonSupervisor,
}

pub(super) struct TurnRouteDecision {
    owner: RouteDecision,
    original_model: Option<String>,
    original_effort: Option<String>,
    original_service_tier: Option<String>,
    original_context_window: Option<String>,
    handoff: Option<(ModelHandoff, TaskClass)>,
    effort: Option<String>,
}

const TURN_EFFORT_CONFIDENCE: f64 = 0.7;
const TURN_EFFORT_INSTRUCTIONS: &str = "Which reasoning effort does nextRequest deserve? \
Answer with the effort that best fits the request's difficulty — but treat the current \
effort as the default: only pick a different level when the task clearly warrants more \
or less reasoning than usual.";

impl TurnRoutePlan {
    /// One bounded background call batches independent effort and model
    /// judgments. An effort answer belongs only to the current model; a
    /// handoff uses the new model's approved or remembered effort instead.
    pub(super) fn evaluate(self) -> Option<TurnRouteDecision> {
        let mut state = json!({
            "sessionId": self.session_id,
            "nextRequest": self.prompt.chars().take(4_000).collect::<String>(),
            "model": self.model,
            "currentEffort": self.current_effort,
        });
        let mut questions = BTreeMap::new();
        if self.efforts.len() >= 2 {
            questions.insert(
                "effort".to_owned(),
                EvalQuestion::Choice {
                    instructions: TURN_EFFORT_INSTRUCTIONS.to_owned(),
                    criteria: self
                        .efforts
                        .iter()
                        .map(|effort| (effort.clone(), None))
                        .collect(),
                },
            );
        }
        if let Some(handoff) = &self.handoff {
            state["previousTurn"] = json!(self.previous_turn);
            state["currentClass"] = json!(handoff.current_class(&self.model));
            state["models"] = json!({"medium": handoff.medium.target, "hard": handoff.hard.target});
            questions.extend(model_handoff_questions());
        }
        if questions.is_empty() {
            return None;
        }
        let payload = self
            .daemon
            .client()
            .request(
                Uuid::nil(),
                self.session_id,
                waku_client::Command::Evaluate {
                    state,
                    questions,
                    feature: Some(
                        if self.handoff.is_some() {
                            "route-handoff"
                        } else {
                            "route-effort"
                        }
                        .to_owned(),
                    ),
                    timeout_secs: None,
                },
            )
            .ok()?;
        let waku_client::ResponsePayload::Evaluation { evaluation } = payload else {
            return None;
        };
        let handoff = self.handoff.and_then(|handoff| {
            let current = handoff.current_class(&self.model)?;
            let class = model_handoff_class(&evaluation, current)?;
            Some((handoff, class))
        });
        let effort = match evaluation.answers.get("effort") {
            Some(EvalAnswer::Choice {
                choice,
                confidence: Some(confidence),
                ..
            }) if handoff.is_none()
                && confidence.is_finite()
                && (TURN_EFFORT_CONFIDENCE..=1.0).contains(confidence)
                && self.efforts.contains(choice)
                && self.current_effort.as_ref() != Some(choice) =>
            {
                Some(choice.clone())
            }
            _ => None,
        };
        if handoff.is_none() && effort.is_none() {
            return None;
        }
        Some(TurnRouteDecision {
            owner: self.owner,
            original_model: self.original_model,
            original_effort: self.original_effort,
            original_service_tier: self.original_service_tier,
            original_context_window: self.original_context_window,
            handoff,
            effort,
        })
    }
}

impl Waku {
    /// The installed, enabled providers Auto may pick between — the same set
    /// the model picker offers a draft on the session's own host.
    pub(super) fn route_candidates_on(&self, key: waku_client::DaemonKey) -> Vec<RouteCandidate> {
        Self::probes_on(&self.probes, key)
            .iter()
            .filter(|probe| {
                probe.installed && !self.disabled_providers_on(key).contains(&probe.provider)
            })
            .map(|probe| RouteCandidate {
                provider: probe.provider,
                models: probe.models.iter().map(|model| model.id.clone()).collect(),
            })
            .collect()
    }

    /// Whether the draft may pick Auto: routing is not disabled, the
    /// session has not started, and at least one provider could take the
    /// route. An unconfigured eval backend does not block it — the daemon
    /// falls back to `last_used` and says so in the decision.
    pub(super) fn auto_route_available(&self) -> bool {
        self.state.auto_model_routing() != AutoModelRouting::Disabled
            && self
                .model_picker_session()
                .is_some_and(|session| !session.provider_locked())
            && !self
                .route_candidates_on(self.model_picker_daemon_key())
                .is_empty()
    }

    /// Auto prompts are configured directly on the Jev page, so the page
    /// stays available before a rule or another eval feature is enabled.
    pub(super) fn jev_in_use(&self) -> bool {
        true
    }

    /// Whether the eval backend on the picked session's daemon is missing
    /// the credential its Jev call requires — the picker's Auto row warns
    /// rather than failing at submit. A daemon that can't be inspected (a
    /// remote host offline) is not a warning: the backend's state is simply
    /// unknown.
    pub(super) fn jev_credential_missing(&self) -> bool {
        let Some(session) = self.model_picker_session() else {
            return false;
        };
        let Some(daemon) = self.daemons.daemon_for_session(session.id) else {
            return false;
        };
        !daemon.settings().eval_ready()
    }

    /// The plan a first Auto submission carries into `prepare_submission`:
    /// the route inputs plus every provider/model-dependent request field
    /// resolved per candidate while probes are cheap to read. `prompt` is
    /// the user-visible text, `provisional_cwd` the path the fallback
    /// request would start in before the workspace materializes.
    pub(super) fn route_start_plan_for_session(
        &self,
        session: &AgentSession,
        prompt: String,
        project: &Project,
        provisional_cwd: PathBuf,
    ) -> Option<RouteStartPlan> {
        let daemon = self.daemons.daemon_for_session(session.id)?;
        let key = self.daemons.session_owner(session.id);
        let candidates = self.route_candidates_on(key);
        let binaries = candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.provider,
                    self.provider_binary_for_session(session.id, candidate.provider),
                )
            })
            .collect();
        let preferred_models = candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.provider,
                    self.provider_probe_on(key, candidate.provider)
                        .and_then(|probe| probe.preferred_model())
                        .map(|model| model.id.clone()),
                )
            })
            .collect();
        Some(RouteStartPlan {
            session_id: session.id,
            prompt,
            project: Some(project.display_name()),
            candidates,
            last_used: Some(RouteTarget {
                provider: self.state.last_provider,
                model: self.state.last_model.clone(),
                effort: self.state.last_reasoning_effort.clone(),
            }),
            daemon,
            event_wake: self.event_wake_tx.clone(),
            binaries,
            preferred_models,
            agent_preset: self.agent_preset_for_session(session),
            deepseek_preferred_preset: self
                .provider_probe_on(key, ProviderKind::DeepSeek)
                .and_then(|probe| probe.preferred_agent_preset())
                .map(|preset| preset.id.clone()),
            provider_cursor: session.provider_cursor.clone(),
            read_own_transcript: session.side_chat_of.is_some()
                || !session.suspended_provider_sessions.is_empty()
                || session.pending_provider_context.is_some(),
            mode: session.runtime_mode,
            computer_use_enabled: self.state.computer_use_enabled,
            remembered_traits: self.state.remembered_model_traits().to_vec(),
            fallback: self.driver_start_request_for_session(session, provisional_cwd),
        })
    }

    /// Snapshot only actionable judgments. Handoffs are opt-in, stay inside
    /// this provider, and require a previous settled turn. Ordinary Medium
    /// tasks retain only their existing effort check.
    pub(super) fn route_turn_plan_for_session(
        &self,
        session: &AgentSession,
        prompt: String,
    ) -> Option<TurnRoutePlan> {
        let owner = session.route_decision.as_ref()?;
        let daemon = self.daemons.daemon_for_session(session.id)?;
        if !daemon.settings().eval_ready() {
            return None;
        }
        let model = self.model_metadata_for_session(session)?;
        let efforts = model
            .reasoning_efforts
            .iter()
            .map(|option| option.id.clone())
            .collect::<Vec<_>>();
        let handoff = self.model_handoff_for_session(session).filter(|_| {
            session
                .turns
                .last()
                .is_some_and(|turn| turn.status != TurnStatus::Running)
        });
        let previous_turn = handoff
            .as_ref()
            .and_then(|_| session.turns.last())
            .map(|turn| {
                let mut state = status_markers::turn_eval_state(session, turn.id, None);
                state["finish"]["success"] = json!(turn.status == TurnStatus::Completed);
                state
            });
        if efforts.len() < 2 && handoff.is_none() {
            return None;
        }
        Some(TurnRoutePlan {
            session_id: session.id,
            prompt,
            model: model.id.clone(),
            efforts,
            current_effort: session
                .reasoning_effort
                .clone()
                .or_else(|| model.default_reasoning_effort.clone()),
            owner: owner.clone(),
            original_model: session.model.clone(),
            original_effort: session.reasoning_effort.clone(),
            original_service_tier: session.service_tier.clone(),
            original_context_window: session.context_window.clone(),
            handoff,
            previous_turn,
            daemon,
        })
    }

    fn model_handoff_for_session(&self, session: &AgentSession) -> Option<ModelHandoff> {
        let probe =
            self.provider_probe_on(self.daemons.session_owner(session.id), session.provider)?;
        let current_model = session.model.as_deref()?;
        model_handoff_targets(
            self.state.phase_routing_enabled,
            session.route_decision.as_ref(),
            session.provider,
            current_model,
            self.state.provider_route_classes.get(&session.provider),
            &self.state.route_classes,
            &probe.models,
        )
    }

    /// Apply only a still-owned decision to the driver that will receive
    /// this prompt. Refusal restores all model traits and records no move.
    pub(super) fn apply_turn_route_decision(
        &mut self,
        session_id: Uuid,
        decision: TurnRouteDecision,
        driver: &waku_client::driver::DriverHandle,
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
        if session.route_decision.as_ref() != Some(&decision.owner)
            || session.model != decision.original_model
            || session.reasoning_effort != decision.original_effort
            || session.service_tier != decision.original_service_tier
            || session.context_window != decision.original_context_window
        {
            return;
        }
        let handoff = decision.handoff.filter(|(expected, _)| {
            self.model_handoff_for_session(session).as_ref() == Some(expected)
        });
        if handoff.is_none() && decision.effort.is_none() {
            return;
        }
        let previous = (
            session.model.clone(),
            session.reasoning_effort.clone(),
            session.service_tier.clone(),
            session.context_window.clone(),
        );
        let provider = session.provider;
        let target = handoff
            .as_ref()
            .and_then(|(handoff, class)| handoff.target(*class).cloned());
        let traits = target
            .as_ref()
            .and_then(|target| target.target.model.as_deref())
            .map(|model| self.state.model_traits_for(provider, model));
        let Some(session) = self.state.session_mut(session_id) else {
            return;
        };
        if let Some(target) = &target {
            session.model.clone_from(&target.target.model);
            let (effort, tier, window) = traits.unwrap_or_default();
            session.reasoning_effort = target.target.effort.clone().or(effort);
            session.service_tier = tier;
            session.context_window = window;
        } else if let Some(effort) = decision.effort {
            session.reasoning_effort = Some(effort);
        }
        let applied = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| self.session_options(session))
            .is_some_and(|options| driver.apply_options(options));
        if !applied {
            if let Some(session) = self.state.session_mut(session_id) {
                (
                    session.model,
                    session.reasoning_effort,
                    session.service_tier,
                    session.context_window,
                ) = previous;
            }
            return;
        }
        let mut class_record = None;
        if let Some(session) = self.state.session_mut(session_id) {
            session.updated_at = unix_time();
            if let Some((_, class)) = handoff {
                let from = previous.0.as_deref().unwrap_or("default");
                let to = session.model.as_deref().unwrap_or("default");
                session.messages.push(Message::new(
                    MessageRole::System,
                    tr!("transcript.model_handoff", from = from, to = to),
                ));
                if let Some(target) = target {
                    class_record = Some((
                        class,
                        target.provider_map,
                        RouteTarget {
                            provider,
                            model: session.model.clone(),
                            effort: session.reasoning_effort.clone(),
                        },
                    ));
                }
            }
        }
        self.state.mark_session_dirty(session_id);
        if let Some((class, provider_map, target)) = class_record {
            self.record_route_class(session_id, class, provider_map, target, cx);
        }
    }

    /// Log that the user replaced an Auto-routed model by hand — the decision
    /// log's `route-override` feature pairs the override with the route that
    /// started the session.
    pub(super) fn record_route_override(
        &self,
        session_id: Uuid,
        provider: ProviderKind,
        model: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(daemon) = self.daemon_for_session(session_id) else {
            return;
        };
        let target = RouteTarget {
            provider,
            model,
            effort: None,
        };
        cx.background_executor()
            .spawn(async move {
                // The override is telemetry: a lost record must not disturb
                // the model change the user just made.
                let _ = daemon.client().request(
                    Uuid::nil(),
                    session_id,
                    waku_client::Command::RecordRouteOverride { session_id, target },
                );
            })
            .detach();
    }

    /// Log that phase routing moved a session onto a class-map target inside
    /// its provider — the `route-class` record pairs with the session-start
    /// `route` record so the settings counts can split mid-session routes
    /// from intake ones. `provider_map` records whether the provider's own
    /// class map or a global entry naming it supplied the target.
    pub(super) fn record_route_class(
        &self,
        session_id: Uuid,
        class: TaskClass,
        provider_map: bool,
        target: RouteTarget,
        cx: &mut Context<Self>,
    ) {
        let Some(daemon) = self.daemon_for_session(session_id) else {
            return;
        };
        cx.background_executor()
            .spawn(async move {
                // Same posture as `record_route_override`: a lost record
                // must not disturb the model move it describes.
                let _ = daemon.client().request(
                    Uuid::nil(),
                    session_id,
                    waku_client::Command::RecordRouteClass {
                        session_id,
                        class,
                        target,
                        provider_map,
                    },
                );
            })
            .detach();
    }
}
