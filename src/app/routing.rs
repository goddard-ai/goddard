//! Auto model routing: a draft whose model selection is Auto asks the
//! daemon's evaluation model to classify its first prompt, and the daemon's
//! routing policy resolves that classification to a concrete provider/model.
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
use uuid::Uuid;
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
    ) -> anyhow::Result<(Option<RouteDecision>, PreparedDriver)> {
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
        let (reasoning_effort, service_tier, context_window) = model
            .as_deref()
            .map(|model| {
                waku_client::persistence::remembered_model_traits_for(
                    &self.remembered_traits,
                    provider,
                    model,
                )
            })
            .unwrap_or_default();
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
                provider_cursor: self.provider_cursor.clone(),
            },
            event_wake: self.event_wake.clone(),
            daemon: self.daemon.clone(),
        })
    }
}

impl Waku {
    /// The installed, enabled providers Auto may pick between — the same set
    /// the model picker offers a draft.
    pub(super) fn route_candidates(&self) -> Vec<RouteCandidate> {
        self.probes
            .iter()
            .filter(|probe| {
                probe.installed && !self.state.disabled_providers.contains(&probe.provider)
            })
            .map(|probe| RouteCandidate {
                provider: probe.provider,
                models: probe.models.iter().map(|model| model.id.clone()).collect(),
            })
            .collect()
    }

    /// Whether the draft may pick Auto: the experiment is on, the session
    /// has not started, and at least one provider could take the route. An
    /// unconfigured eval backend does not block it — the daemon falls back
    /// to `last_used` and says so in the decision.
    pub(super) fn auto_route_available(&self) -> bool {
        self.state.model_router_enabled
            && self
                .composer_session()
                .is_some_and(|session| !session.provider_locked())
            && !self.route_candidates().is_empty()
    }

    /// Whether the selected eval backend is missing the credential its Jev
    /// call requires — the picker's Auto row warns rather than failing at
    /// submit. Mirrors the `required` checks in waku-core's `backend_request`.
    pub(super) fn jev_credential_missing(&self) -> bool {
        let eval = self.state.eval.clone().unwrap_or_default();
        let missing = |value: &Option<String>| {
            value.as_deref().map(str::trim).unwrap_or_default().is_empty()
        };
        match eval.backend {
            waku_protocol::eval::EvalBackend::TypeSafe => missing(&eval.typesafe_api_key),
            waku_protocol::eval::EvalBackend::VercelGateway => missing(&eval.vercel_api_key),
            waku_protocol::eval::EvalBackend::Cloudflare => {
                missing(&eval.cloudflare_account_id) || missing(&eval.cloudflare_api_token)
            }
        }
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
        let candidates = self.route_candidates();
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
                    self.provider_probe(candidate.provider)
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
            }),
            daemon,
            event_wake: self.event_wake_tx.clone(),
            binaries,
            preferred_models,
            agent_preset: self.agent_preset_for_session(session),
            deepseek_preferred_preset: self
                .provider_probe(ProviderKind::DeepSeek)
                .and_then(|probe| probe.preferred_agent_preset())
                .map(|preset| preset.id.clone()),
            provider_cursor: session.provider_cursor.clone(),
            mode: session.runtime_mode,
            computer_use_enabled: self.state.computer_use_enabled,
            remembered_traits: self.state.remembered_model_traits().to_vec(),
            fallback: self.driver_start_request_for_session(session, provisional_cwd),
        })
    }

    /// Fetch the daemon's routing policy for the settings surface. The answer
    /// lands through the event pump like every other async daemon result.
    pub(super) fn request_route_policy(&mut self, cx: &mut Context<Self>) {
        if self.route_policy_pending {
            return;
        }
        self.route_policy_pending = true;
        let tx = self.route_policy_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        let daemon = self.daemon.client();
        cx.background_executor()
            .spawn(async move {
                let result = daemon
                    .request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::GetRoutePolicy,
                    )
                    .map_err(|error| format!("{error:#}"))
                    .and_then(|payload| match payload {
                        waku_client::ResponsePayload::RoutePolicy { view } => Ok(view),
                        _ => Err("the daemon returned an invalid route policy response".into()),
                    });
                if tx.send(result).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .detach();
    }

    /// Write one class-level target into the policy document, then refresh
    /// the cached view — the file stays the source of truth, so a failed
    /// write simply leaves the previous view on screen.
    pub(super) fn set_route_class_target(
        &mut self,
        class: TaskClass,
        target: String,
        cx: &mut Context<Self>,
    ) {
        self.route_policy_pending = true;
        let tx = self.route_policy_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        let daemon = self.daemon.client();
        cx.background_executor()
            .spawn(async move {
                let result = daemon
                    .request(
                        Uuid::nil(),
                        Uuid::nil(),
                        waku_client::Command::SetRouteClassTarget { class, target },
                    )
                    .map_err(|error| format!("{error:#}"))
                    .and_then(|payload| match payload {
                        waku_client::ResponsePayload::Ack => daemon
                            .request(
                                Uuid::nil(),
                                Uuid::nil(),
                                waku_client::Command::GetRoutePolicy,
                            )
                            .map_err(|error| format!("{error:#}"))
                            .and_then(|payload| match payload {
                                waku_client::ResponsePayload::RoutePolicy { view } => Ok(view),
                                _ => {
                                    Err("the daemon returned an invalid route policy response"
                                        .into())
                                }
                            }),
                        _ => Err("the daemon returned an invalid route policy response".into()),
                    });
                if tx.send(result).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .detach();
    }

    /// Land fetched policy views. Drained by the event pump; a write that
    /// failed leaves the previous view in place and surfaces nothing — the
    /// dropdown simply stays on the file's last-known value.
    pub(super) fn drain_route_policy_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.route_policy_events.try_recv() {
            self.route_policy_pending = false;
            match result {
                Ok(view) => {
                    changed |= self.route_policy.as_ref() != Some(&view);
                    self.route_policy = Some(view);
                }
                Err(error) => {
                    eprintln!("Goddard: route policy request failed: {error}");
                }
            }
        }
        changed
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
        let target = RouteTarget { provider, model };
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
}
