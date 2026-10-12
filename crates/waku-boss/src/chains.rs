//! Transactional Phase 1 safety reducer. All mutations use BossService::update:
//! a rejected or failed durable write leaves the in-memory document unchanged.
//! No reducer operation launches a provider or transfers a workspace.
use crate::BossService;
use anyhow::{Result, anyhow, bail, ensure};
use uuid::Uuid;
use waku_protocol::{
    AgentWorkspace,
    boss::{BossOutcome, BossState, OutcomeState, OutcomeTransition, PlanActor},
    chains::*,
};

const SCHEMA_VERSION: u32 = 1;
const ADMISSION_PENDING: &str = "awaiting daemon admission";

/// Trusted host observations, deliberately absent from the wire operation.
/// A result producer cannot certify its own shutdown, access, or landing.
#[derive(Clone, Debug)]
pub enum ChainObservation {
    Admitted {
        employee_id: Uuid,
    },
    Starting,
    Working {
        starting_revision: String,
        workspace: String,
    },
    Idle,
    LaunchError {
        reason: String,
    },
    Terminal {
        terminal: ChainTerminal,
    },
    EffectsReconciled {
        source_revision: String,
    },
    Released {
        execution: bool,
        workspace: bool,
    },
}

/// Facts the host checked under the successor's existing grants. Missing facts
/// pause; these fields are not employee assertions and never widen access.
#[derive(Default)]
pub struct HandoffVerification {
    pub accessible_artifacts: Vec<String>,
    pub clean_committed_workspace: Option<(String, String)>,
    pub confirmed_landing: Option<(String, String, String)>,
    pub source_revision: Option<String>,
    pub successor_access: bool,
}

fn same<T: serde::Serialize>(left: &T, right: &T) -> Result<bool> {
    Ok(serde_json::to_vec(left)? == serde_json::to_vec(right)?)
}
fn live(chain: &EmployeeChain) -> bool {
    !matches!(
        chain.status,
        ChainStatus::Completed | ChainStatus::Cancelled
    )
}
fn current_attempt(
    chain: &mut EmployeeChain,
    attempt: Uuid,
    version: u64,
) -> Result<&mut ChainAttempt> {
    let entry = chain.steps[chain.current_step]
        .attempts
        .last_mut()
        .ok_or_else(|| anyhow!("step has no attempt"))?;
    ensure!(
        entry.id == attempt && entry.definition_version == version,
        "stale chain attempt"
    );
    Ok(entry)
}
fn chain_mut(state: &mut BossState, id: Uuid) -> Result<&mut EmployeeChain> {
    state
        .chains
        .iter_mut()
        .find(|chain| chain.id == id)
        .ok_or_else(|| anyhow!("unknown chain {id}"))
}
fn actor(caller: Option<Uuid>) -> ChainActor {
    caller.map_or(ChainActor::Human, |session_id| ChainActor::Session {
        session_id,
    })
}

fn audit(chain: &mut EmployeeChain, actor: ChainActor, event_id: Uuid, event: &str) -> Result<()> {
    chain.revision = chain
        .revision
        .checked_add(1)
        .ok_or_else(|| anyhow!("chain revision exhausted"))?;
    chain.history.push(ChainHistory {
        event_id,
        actor,
        step: chain.current_step,
        attempt_id: chain.steps[chain.current_step]
            .attempts
            .last()
            .map(|attempt| attempt.id),
        definition_version: chain.definition_version,
        revision: chain.revision,
        event: event.into(),
        at: waku_protocol::model::unix_time(),
    });
    Ok(())
}
fn pause(chain: &mut EmployeeChain, reason: impl Into<String>) {
    let reason = reason.into();
    chain.status = ChainStatus::Paused;
    chain.reason = Some(reason.clone());
    if chain
        .attention
        .as_ref()
        .is_none_or(|attention| attention.handled_at.is_some())
    {
        chain.attention = Some(ChainAttention {
            id: Uuid::new_v4(),
            reason,
            handled_at: None,
        });
    }
}
fn new_attempt(version: u64) -> ChainAttempt {
    ChainAttempt {
        id: Uuid::new_v4(),
        dispatch_id: Uuid::new_v4(),
        definition_version: version,
        employee_id: None,
        status: StepStatus::Waiting,
        starting_revision: None,
        workspace: None,
        execution_released: false,
        workspace_released: false,
        provisional_result: None,
        terminal: None,
        sealed: None,
        handoff_accepted: false,
        reconciled_retry_revision: None,
        created_at: waku_protocol::model::unix_time(),
    }
}
fn unique_names(names: &[String], kind: &str) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for name in names {
        ensure!(
            !name.trim().is_empty() && seen.insert(name),
            "empty or duplicate {kind}"
        );
    }
    Ok(())
}
fn validate_step(
    state: &BossState,
    step: &ChainStepDefinition,
    predecessor: Option<HandoffBoundary>,
) -> Result<()> {
    ensure!(
        !step.title.trim().is_empty() && !step.prompt.trim().is_empty(),
        "step title and prompt are required"
    );
    ensure!(!step.project_id.is_nil(), "step project is required");
    ensure!(
        !step.model.trim().is_empty()
            && step.model == step.model.trim()
            && !matches!(step.model.as_str(), "auto" | "default"),
        "step model must be concrete"
    );
    if let Some(persona) = step.persona_id {
        ensure!(
            state
                .personas
                .iter()
                .any(|entry| entry.id == persona && entry.id != state.persona_id),
            "unknown employee persona"
        );
    }
    ensure!(
        state.employee_persona_id.is_some(),
        "canonical Employee persona is unresolved"
    );
    ensure!(
        step.item_id.is_none() || step.plan_id.is_some(),
        "item requires a plan"
    );
    if let Some(plan_id) = step.plan_id {
        let plan = state
            .planning
            .iter()
            .find(|plan| plan.id == plan_id)
            .ok_or_else(|| anyhow!("unknown chain plan"))?;
        ensure!(
            plan.finalized_at.is_some(),
            "chain implementation requires an approved plan"
        );
        if let Some(item_id) = step.item_id {
            ensure!(
                plan.items.iter().any(|item| item.id == item_id),
                "unknown plan item"
            );
        }
    }
    ensure!(
        step.workspace != AgentWorkspace::Adopt,
        "adoption is resolved only from a validated committed handoff"
    );
    // A step that adopts a committed workspace — or starts fresh at a landed
    // revision — takes its base from the verified handoff, not the definition.
    let inherited_base = matches!(
        predecessor,
        Some(HandoffBoundary::CommittedWorkspace | HandoffBoundary::Landed)
    );
    if step.workspace == AgentWorkspace::Worktree && !inherited_base {
        ensure!(
            step.base_revision
                .as_ref()
                .is_some_and(|revision| !revision.trim().is_empty()),
            "worktree requires a source revision"
        );
    }
    unique_names(&step.required_checks, "check")?;
    unique_names(&step.required_artifacts, "artifact")?;
    unique_names(&step.required_approvals, "approval")?;
    Ok(())
}
fn validate_definition(state: &BossState, definition: &ChainDefinition) -> Result<()> {
    ensure!(
        !definition.steps.is_empty(),
        "a linear chain needs at least one step"
    );
    let outcome = state
        .outcomes
        .iter()
        .find(|outcome| outcome.id == definition.outcome_id)
        .ok_or_else(|| anyhow!("unknown chain outcome"))?;
    ensure!(!outcome.terminal(), "chain outcome is closed");
    for (index, step) in definition.steps.iter().enumerate() {
        validate_step(
            state,
            step,
            index
                .checked_sub(1)
                .map(|prior| definition.steps[prior].boundary),
        )?;
    }
    validate_workspace_run(definition.steps.iter())?;
    if definition.finishes_outcome {
        ensure!(
            !outcome.success_criteria.trim().is_empty(),
            "finisher requires explicit success criteria"
        );
        ensure!(
            !state
                .chains
                .iter()
                .any(|chain| chain.request.outcome_id == definition.outcome_id
                    && chain.finisher_reserved),
            "outcome already has a chain finisher"
        );
        ensure!(
            !outcome
                .finishing_assignment
                .is_some_and(|id| state
                    .employees
                    .iter()
                    .any(|employee| employee.session_id == id
                        && !employee.expired
                        && !employee.cancelled)),
            "outcome already has a live finisher"
        );
    }
    Ok(())
}

/// A committed workspace is staging, not a boundary: the unit is submitted
/// only when a later step in the same project declares Landed — that step's
/// employee adopts the accumulated worktree and runs the merge submit. Every
/// step in between must keep handing the same worktree forward, so it also
/// declares CommittedWorkspace inside that project.
fn validate_workspace_run<'a>(steps: impl Iterator<Item = &'a ChainStepDefinition>) -> Result<()> {
    let steps: Vec<&ChainStepDefinition> = steps.collect();
    for (index, step) in steps.iter().enumerate() {
        if step.boundary != HandoffBoundary::CommittedWorkspace {
            continue;
        }
        let mut submitted = false;
        for next in &steps[index + 1..] {
            ensure!(
                next.project_id == step.project_id,
                "a committed workspace stays inside its own project"
            );
            ensure!(
                next.workspace == AgentWorkspace::Worktree,
                "a committed-workspace successor must declare a managed worktree; adoption is resolved at handoff"
            );
            if next.boundary == HandoffBoundary::Landed {
                submitted = true;
                break;
            }
            ensure!(
                next.boundary == HandoffBoundary::CommittedWorkspace,
                "a committed workspace must reach its submitting step without interruption"
            );
        }
        ensure!(
            submitted,
            "a committed-workspace handoff must name the step that submits the final unit"
        );
    }
    Ok(())
}

impl BossService {
    /// Shared client/scripting contract. Only a bound attempt can write its
    /// provisional result; chain controls never inherit employee permissions.
    pub fn chain_operation(
        &self,
        caller: Option<Uuid>,
        action: ChainOperation,
    ) -> Result<EmployeeChain> {
        let owner = caller.is_none_or(|caller| self.has_boss_authority(caller));
        if !owner {
            ensure!(
                matches!(action, ChainOperation::RecordResult { .. }),
                "only the boss or a human can inspect or control chains"
            );
            self.require_active(caller.ok_or_else(|| anyhow!("missing employee identity"))?)?;
        }
        let mut result = None;
        let mutating = !matches!(action, ChainOperation::Show { .. });
        self.update(|state| {
            let now = waku_protocol::model::unix_time();
            match action {
                ChainOperation::Create {
                    request_id,
                    definition,
                } => {
                    ensure!(!request_id.is_nil(), "creation request id is required");
                    if let Some(existing) = state
                        .chains
                        .iter()
                        .find(|chain| chain.request_id == request_id)
                    {
                        ensure!(
                            existing.supervisor_id == caller
                                && same(&existing.request, &definition)?,
                            "conflicting chain request id reuse"
                        );
                        result = Some(existing.clone());
                        return Ok(());
                    }
                    validate_definition(state, &definition)?;
                    for step in &definition.steps {
                        self.validate_bucket_ids(&step.permissions.bucket_ids)?;
                    }
                    let mut steps = definition
                        .steps
                        .iter()
                        .cloned()
                        .map(|definition| ChainStep {
                            definition,
                            status: StepStatus::NotStarted,
                            attempts: Vec::new(),
                            approvals: Vec::new(),
                        })
                        .collect::<Vec<_>>();
                    steps[0].status = StepStatus::Waiting;
                    steps[0].attempts.push(new_attempt(1));
                    let mut chain = EmployeeChain {
                        schema_version: SCHEMA_VERSION,
                        id: Uuid::new_v4(),
                        request_id,
                        supervisor_id: caller,
                        finisher_reserved: definition.finishes_outcome,
                        request: definition,
                        definition_version: 1,
                        revision: 0,
                        current_step: 0,
                        status: ChainStatus::Waiting,
                        reason: Some(ADMISSION_PENDING.into()),
                        steps,
                        attention: None,
                        history: Vec::new(),
                    };
                    audit(&mut chain, actor(caller), request_id, "created")?;
                    result = Some(chain.clone());
                    state.chains.push(chain);
                }
                ChainOperation::Show { chain_id } => {
                    result = Some(chain_mut(state, chain_id)?.clone());
                }
                ChainOperation::RecordResult {
                    chain_id,
                    attempt_id,
                    definition_version,
                    result: packet,
                } => {
                    let chain = chain_mut(state, chain_id)?;
                    let attempt = current_attempt(chain, attempt_id, definition_version)?;
                    ensure!(
                        owner || attempt.employee_id == caller,
                        "result belongs to a different employee"
                    );
                    if let Some(existing) = &attempt.provisional_result {
                        ensure!(existing == &packet, "conflicting result for this attempt");
                    } else {
                        ensure!(attempt.sealed.is_none(), "sealed evidence is immutable");
                        unique_names(
                            &packet
                                .checks
                                .iter()
                                .map(|check| check.name.clone())
                                .collect::<Vec<_>>(),
                            "result check",
                        )?;
                        unique_names(
                            &packet
                                .artifacts
                                .iter()
                                .map(|artifact| artifact.name.clone())
                                .collect::<Vec<_>>(),
                            "result artifact",
                        )?;
                        attempt.provisional_result = Some(packet);
                        audit(
                            chain,
                            actor(caller),
                            Uuid::new_v4(),
                            "provisional result recorded",
                        )?;
                    }
                    // The daemon returns Saved for this write, withholding other assignments.
                    result = Some(chain.clone());
                }
                ChainOperation::Control {
                    chain_id,
                    expected_revision,
                    control,
                    source_revision,
                } => {
                    let chain = chain_mut(state, chain_id)?;
                    ensure!(chain.revision == expected_revision, "stale chain revision");
                    ensure!(live(chain), "chain is terminal");
                    ensure!(
                        control == ChainControl::Retry || source_revision.is_none(),
                        "source revision applies only to retry"
                    );
                    let step = &mut chain.steps[chain.current_step];
                    let released = step.attempts.last().is_none_or(|attempt| {
                        attempt.employee_id.is_none()
                            || (attempt.execution_released && attempt.workspace_released)
                    });
                    audit(
                        chain,
                        actor(caller),
                        Uuid::new_v4(),
                        &format!("control {control:?}"),
                    )?;
                    let step = &mut chain.steps[chain.current_step];
                    match control {
                        ChainControl::Pause => {
                            chain.status = if released {
                                ChainStatus::Paused
                            } else {
                                ChainStatus::Pausing
                            };
                            chain.reason = Some("paused by owner".into());
                        }
                        ChainControl::Cancel => {
                            chain.status = if released {
                                ChainStatus::Cancelled
                            } else {
                                ChainStatus::Cancelling
                            };
                            chain.reason = Some("cancelled by owner; work preserved".into());
                            if released {
                                step.status = StepStatus::Cancelled;
                                chain.finisher_reserved = false;
                            }
                        }
                        ChainControl::Resume => {
                            ensure!(
                                chain.status == ChainStatus::Paused,
                                "resume requires a paused chain"
                            );
                            ensure!(released, "shutdown must be confirmed before resume");
                            if step.status == StepStatus::Succeeded {
                                advance(chain)?;
                            } else if step.attempts.last().is_some_and(|attempt| {
                                attempt.sealed.is_some() || attempt.employee_id.is_some()
                            }) {
                                bail!("retry required; resume never reruns an interrupted attempt");
                            } else {
                                chain.status = ChainStatus::Waiting;
                                chain.reason = Some(ADMISSION_PENDING.into());
                            }
                        }
                        ChainControl::Retry => {
                            ensure!(
                                chain.status == ChainStatus::Paused && released,
                                "retry requires a paused, released attempt"
                            );
                            let previous = step
                                .attempts
                                .last()
                                .ok_or_else(|| anyhow!("no attempt to retry"))?;
                            ensure!(
                                previous
                                    .sealed
                                    .as_ref()
                                    .is_some_and(|handoff| !handoff.accepted),
                                "prior effects must be reconciled before retry"
                            );
                            ensure!(
                                step.attempts.len() <= step.definition.max_retries as usize,
                                "retry limit reached"
                            );
                            // The old workspace is retained, never adopted or reset implicitly.
                            let revision = source_revision
                                .filter(|revision| !revision.trim().is_empty())
                                .ok_or_else(|| {
                                    anyhow!(
                                        "retry requires explicit fresh-workspace source revision"
                                    )
                                })?;
                            ensure!(
                                previous.reconciled_retry_revision.as_ref() == Some(&revision),
                                "uncertain prior effects must be reconciled before retry"
                            );
                            let mut attempt = new_attempt(chain.definition_version);
                            attempt.starting_revision = Some(revision);
                            step.attempts.push(attempt);
                            step.status = StepStatus::Waiting;
                            chain.status = ChainStatus::Waiting;
                            chain.reason = Some(ADMISSION_PENDING.into());
                        }
                    }
                    if matches!(
                        control,
                        ChainControl::Resume | ChainControl::Retry | ChainControl::Cancel
                    ) {
                        if let Some(attention) = &mut chain.attention {
                            attention.handled_at = Some(now);
                        }
                    }
                    result = Some(chain.clone());
                }
                ChainOperation::Revise {
                    chain_id,
                    expected_revision,
                    expected_definition_version,
                    step,
                    definition,
                } => {
                    self.validate_bucket_ids(&definition.permissions.bucket_ids)?;
                    let predecessor = state
                        .chains
                        .iter()
                        .find(|chain| chain.id == chain_id)
                        .and_then(|chain| {
                            step.checked_sub(1).and_then(|prior| chain.steps.get(prior))
                        })
                        .map(|prior| prior.definition.boundary);
                    validate_step(state, &definition, predecessor)?;
                    let chain = chain_mut(state, chain_id)?;
                    ensure!(
                        chain.revision == expected_revision
                            && chain.definition_version == expected_definition_version,
                        "stale chain revision or definition"
                    );
                    ensure!(
                        chain.status == ChainStatus::Paused,
                        "revise requires a paused chain"
                    );
                    let target = chain
                        .steps
                        .get_mut(step)
                        .ok_or_else(|| anyhow!("unknown chain step"))?;
                    ensure!(
                        target.status == StepStatus::NotStarted && target.attempts.is_empty(),
                        "accepted steps cannot be revised"
                    );
                    target.definition = definition;
                    validate_workspace_run(chain.steps.iter().map(|step| &step.definition))?;
                    chain.definition_version = chain
                        .definition_version
                        .checked_add(1)
                        .ok_or_else(|| anyhow!("definition version exhausted"))?;
                    // Active attempts retain their version. Future revisions must not make their
                    // terminal events stale; the event gate checks the attempt's own snapshot.
                    audit(
                        chain,
                        actor(caller),
                        Uuid::new_v4(),
                        "unstarted step revised",
                    )?;
                    result = Some(chain.clone());
                }
                ChainOperation::Approve {
                    chain_id,
                    expected_revision,
                    gate,
                    approved,
                } => {
                    ensure!(
                        caller.is_none(),
                        "chain approval requires an explicit human decision"
                    );
                    let chain = chain_mut(state, chain_id)?;
                    ensure!(chain.revision == expected_revision, "stale chain revision");
                    ensure!(
                        chain.status == ChainStatus::Paused,
                        "approval requires a paused chain"
                    );
                    let step = &mut chain.steps[chain.current_step];
                    ensure!(
                        step.definition.required_approvals.contains(&gate),
                        "unknown approval gate"
                    );
                    if !approved && let Some(attempt) = step.attempts.last_mut() {
                        attempt.handoff_accepted = false;
                        if attempt.sealed.is_some() {
                            attempt.status = StepStatus::Failed;
                            step.status = StepStatus::Failed;
                        }
                    }
                    step.approvals.push(ChainApproval {
                        gate,
                        actor: ChainActor::Human,
                        approved,
                        definition_version: step
                            .attempts
                            .last()
                            .map_or(chain.definition_version, |attempt| {
                                attempt.definition_version
                            }),
                        at: now,
                    });
                    audit(
                        chain,
                        actor(caller),
                        Uuid::new_v4(),
                        "approval recorded; explicit resume required",
                    )?;
                    result = Some(chain.clone());
                }
            }
            // Chain work is outcome activity — an executing chain must not
            // read as unattended to the outcome reminder scan.
            if mutating
                && let Some(chain) = &result
                && let Some(outcome) = state
                    .outcomes
                    .iter_mut()
                    .find(|outcome| outcome.id == chain.request.outcome_id)
            {
                outcome.last_activity_at = now;
            }
            Ok(())
        })?;
        result.ok_or_else(|| anyhow!("chain operation produced no result"))
    }

    /// Recheck repaired access/approval gates against immutable sealed evidence.
    /// Acceptance is a new audited decision, never an edit to the producer packet.
    pub fn recheck_chain_handoff(
        &self,
        chain_id: Uuid,
        attempt_id: Uuid,
        version: u64,
        verification: &HandoffVerification,
    ) -> Result<()> {
        self.update(|state| {
            let parent_open = state
                .chains
                .iter()
                .find(|chain| chain.id == chain_id)
                .and_then(|chain| {
                    state
                        .outcomes
                        .iter()
                        .find(|outcome| outcome.id == chain.request.outcome_id)
                })
                .is_some_and(|outcome| !outcome.terminal());
            ensure!(parent_open, "chain outcome missing or closed");
            let chain = chain_mut(state, chain_id)?;
            ensure!(
                chain.status == ChainStatus::Paused,
                "gate repair requires a paused chain"
            );
            let attempt = current_attempt(chain, attempt_id, version)?;
            if attempt.handoff_accepted {
                return Ok(());
            }
            let handoff = attempt
                .sealed
                .as_ref()
                .ok_or_else(|| anyhow!("terminal evidence is not sealed"))?;
            ensure!(
                attempt
                    .terminal
                    .as_ref()
                    .is_some_and(|terminal| terminal.disposition == ResultDisposition::Success)
                    && handoff.result.disposition == ResultDisposition::Success,
                "failed execution requires retry"
            );
            let packet = handoff.result.clone();
            if let Some(reason) = gate_reason(
                &chain.steps[chain.current_step],
                &packet,
                verification,
                version,
            ) {
                pause(chain, reason);
            } else {
                let attempt = current_attempt(chain, attempt_id, version)?;
                attempt.handoff_accepted = true;
                attempt.status = StepStatus::Succeeded;
                chain.steps[chain.current_step].status = StepStatus::Succeeded;
                chain.reason = Some("handoff accepted; explicit resume required".into());
            }
            let outcome_id = chain.request.outcome_id;
            let result = audit(
                chain,
                ChainActor::Daemon,
                Uuid::new_v4(),
                "sealed handoff gates rechecked",
            );
            if result.is_ok()
                && let Some(outcome) = state
                    .outcomes
                    .iter_mut()
                    .find(|outcome| outcome.id == outcome_id)
            {
                outcome.last_activity_at = waku_protocol::model::unix_time();
            }
            result
        })
    }

    /// Host-only observation entry point. Stable event IDs deduplicate delivery.
    /// Admission binding commits before launch; a lost response reads that binding.
    pub fn observe_chain(
        &self,
        chain_id: Uuid,
        attempt_id: Uuid,
        version: u64,
        event_id: Uuid,
        observation: ChainObservation,
    ) -> Result<()> {
        self.update(|state| {
            let parent_open = state
                .chains
                .iter()
                .find(|chain| chain.id == chain_id)
                .and_then(|chain| {
                    state
                        .outcomes
                        .iter()
                        .find(|outcome| outcome.id == chain.request.outcome_id)
                })
                .is_some_and(|outcome| !outcome.terminal());
            let chain = chain_mut(state, chain_id)?;
            if chain.history.iter().any(|entry| entry.event_id == event_id) {
                return Ok(());
            }
            ensure!(
                live(chain)
                    || matches!(
                        observation,
                        ChainObservation::Terminal { .. }
                            | ChainObservation::LaunchError { .. }
                            | ChainObservation::Released { .. }
                    ),
                "chain is terminal"
            );
            if !parent_open
                && !matches!(
                    observation,
                    ChainObservation::Terminal { .. }
                        | ChainObservation::LaunchError { .. }
                        | ChainObservation::Released { .. }
                )
            {
                pause(chain, "chain outcome missing or closed");
                return audit(chain, ChainActor::Daemon, event_id, "parent unavailable");
            }
            let stopping = matches!(
                chain.status,
                ChainStatus::Pausing
                    | ChainStatus::Cancelling
                    | ChainStatus::Paused
                    | ChainStatus::Cancelled
            );
            let attempt = current_attempt(chain, attempt_id, version)?;
            ensure!(
                attempt.sealed.is_none()
                    || matches!(observation, ChainObservation::EffectsReconciled { .. }),
                "sealed attempt is immutable"
            );
            if matches!(
                observation,
                ChainObservation::Admitted { .. }
                    | ChainObservation::Starting
                    | ChainObservation::Working { .. }
                    | ChainObservation::Idle
            ) {
                ensure!(
                    !attempt.execution_released && attempt.terminal.is_none(),
                    "attempt execution already ended"
                );
            }
            match &observation {
                ChainObservation::Admitted { employee_id } => {
                    ensure!(!stopping, "chain dispatch is stopped");
                    ensure!(!employee_id.is_nil(), "employee identity is required");
                    ensure!(
                        attempt
                            .employee_id
                            .is_none_or(|existing| existing == *employee_id),
                        "attempt already bound to another employee"
                    );
                    attempt.employee_id = Some(*employee_id);
                    attempt.status = StepStatus::Queued;
                }
                ChainObservation::Starting => {
                    ensure!(
                        !stopping
                            && attempt.employee_id.is_some()
                            && attempt.status == StepStatus::Queued,
                        "attempt is not eligible to start"
                    );
                    attempt.status = StepStatus::Starting;
                }
                ChainObservation::Working {
                    starting_revision,
                    workspace,
                } => {
                    ensure!(
                        !stopping
                            && attempt.employee_id.is_some()
                            && matches!(attempt.status, StepStatus::Starting | StepStatus::Working),
                        "execution not preceded by dispatch"
                    );
                    ensure!(
                        !starting_revision.is_empty() && !workspace.is_empty(),
                        "observed workspace and revision required"
                    );
                    ensure!(
                        attempt
                            .starting_revision
                            .as_ref()
                            .is_none_or(|existing| existing == starting_revision)
                            && attempt
                                .workspace
                                .as_ref()
                                .is_none_or(|existing| existing == workspace),
                        "attempt workspace changed"
                    );
                    attempt.starting_revision = Some(starting_revision.clone());
                    attempt.workspace = Some(workspace.clone());
                    attempt.status = StepStatus::Working;
                }
                ChainObservation::Idle => {
                    ensure!(
                        attempt.employee_id.is_some(),
                        "idle requires an admitted employee"
                    );
                    attempt.status = StepStatus::WaitingForEmployee;
                }
                ChainObservation::LaunchError { reason } => {
                    attempt.terminal = Some(ChainTerminal {
                        message: reason.clone(),
                        disposition: ResultDisposition::Failed,
                    });
                    attempt.status = StepStatus::CheckingHandoff;
                }
                ChainObservation::Terminal { terminal } => {
                    ensure!(
                        attempt.employee_id.is_some(),
                        "terminal execution requires an admitted employee"
                    );
                    if let Some(existing) = &attempt.terminal {
                        ensure!(existing == terminal, "contradictory terminal observation");
                    }
                    attempt.terminal = Some(terminal.clone());
                    attempt.status = StepStatus::CheckingHandoff;
                }
                ChainObservation::EffectsReconciled { source_revision } => {
                    ensure!(
                        attempt.execution_released
                            && attempt.workspace_released
                            && attempt
                                .sealed
                                .as_ref()
                                .is_some_and(|handoff| !handoff.accepted),
                        "recovery requires a sealed released unsuccessful attempt"
                    );
                    ensure!(
                        !source_revision.trim().is_empty(),
                        "reconciled source revision is required"
                    );
                    attempt.reconciled_retry_revision = Some(source_revision.clone());
                }
                ChainObservation::Released {
                    execution,
                    workspace,
                } => {
                    attempt.execution_released |= execution;
                    attempt.workspace_released |= workspace;
                }
            }
            let status = attempt.status;
            let released = attempt.execution_released && attempt.workspace_released;
            chain.steps[chain.current_step].status = status;
            if stopping {
                if released && chain.status == ChainStatus::Pausing {
                    chain.status = ChainStatus::Paused;
                }
                if released && chain.status == ChainStatus::Cancelling {
                    chain.status = ChainStatus::Cancelled;
                    chain.finisher_reserved = false;
                    chain.steps[chain.current_step].status = StepStatus::Cancelled;
                }
            } else {
                chain.status = match status {
                    StepStatus::Queued => ChainStatus::Queued,
                    StepStatus::Starting => ChainStatus::Starting,
                    StepStatus::Working => ChainStatus::Working,
                    StepStatus::WaitingForEmployee => ChainStatus::WaitingForEmployee,
                    StepStatus::CheckingHandoff => ChainStatus::CheckingHandoff,
                    _ => chain.status,
                };
                chain.reason = None;
            }
            let event = match observation {
                ChainObservation::Admitted { .. } => "admitted",
                ChainObservation::Starting => "starting",
                ChainObservation::Working { .. } => "working",
                ChainObservation::Idle => "idle",
                ChainObservation::LaunchError { .. } => "launch error",
                ChainObservation::Terminal { .. } => "terminal",
                ChainObservation::Released { .. } => "ownership released",
                ChainObservation::EffectsReconciled { .. } => "prior effects reconciled",
            };
            let outcome_id = chain.request.outcome_id;
            let result = audit(chain, ChainActor::Daemon, event_id, event);
            if result.is_ok()
                && let Some(outcome) = state
                    .outcomes
                    .iter_mut()
                    .find(|outcome| outcome.id == outcome_id)
            {
                outcome.last_activity_at = waku_protocol::model::unix_time();
            }
            result
        })
    }

    /// Seal terminal text and evidence only after both ownership releases. The
    /// acceptance decision and successor eligibility commit in the same document.
    pub fn seal_chain_handoff(
        &self,
        chain_id: Uuid,
        attempt_id: Uuid,
        version: u64,
        verification: &HandoffVerification,
    ) -> Result<()> {
        self.update(|state| {
            let parent_open = state
                .chains
                .iter()
                .find(|chain| chain.id == chain_id)
                .and_then(|chain| {
                    state
                        .outcomes
                        .iter()
                        .find(|outcome| outcome.id == chain.request.outcome_id)
                })
                .is_some_and(|outcome| !outcome.terminal());
            let chain = chain_mut(state, chain_id)?;
            if chain
                .steps
                .iter()
                .flat_map(|step| &step.attempts)
                .any(|attempt| {
                    attempt.id == attempt_id
                        && attempt.definition_version == version
                        && attempt.sealed.is_some()
                })
            {
                return Ok(());
            }
            let attempt = current_attempt(chain, attempt_id, version)?;
            ensure!(
                attempt.execution_released && attempt.workspace_released,
                "execution and workspace release are not confirmed"
            );
            let terminal = attempt
                .terminal
                .clone()
                .ok_or_else(|| anyhow!("terminal history unavailable"))?;
            let mut packet = attempt.provisional_result.clone().unwrap_or(ChainResult {
                disposition: ResultDisposition::Failed,
                checks: Vec::new(),
                artifacts: Vec::new(),
                code: None,
                limitations: vec!["structured result missing".into()],
            });
            let mut reason = terminal
                .message
                .trim()
                .is_empty()
                .then(|| "terminal message is unavailable".to_owned());
            if terminal.disposition != ResultDisposition::Success {
                packet.disposition = terminal.disposition;
                reason = Some("terminal execution did not succeed".to_owned());
            } else if packet.disposition != ResultDisposition::Success {
                reason = Some("structured result missing or unsuccessful".to_owned());
            }
            if reason.is_none() && !parent_open {
                reason = Some("chain outcome missing or closed".into());
            }
            if reason.is_none() {
                reason = gate_reason(
                    &chain.steps[chain.current_step],
                    &packet,
                    verification,
                    version,
                );
            }
            let accepted = reason.is_none();
            let successor =
                (chain.current_step + 1 < chain.steps.len()).then_some(chain.current_step + 1);
            let attempt = current_attempt(chain, attempt_id, version)?;
            attempt.sealed = Some(ChainHandoff {
                producer_attempt: attempt_id,
                definition_version: version,
                terminal_message: terminal.message,
                result: packet,
                successor_step: successor,
                accepted,
                reason: reason.clone(),
                sealed_at: waku_protocol::model::unix_time(),
            });
            attempt.handoff_accepted = accepted;
            attempt.status = if accepted {
                StepStatus::Succeeded
            } else {
                StepStatus::Failed
            };
            chain.steps[chain.current_step].status = attempt.status;
            let held = matches!(
                chain.status,
                ChainStatus::Paused
                    | ChainStatus::Pausing
                    | ChainStatus::Cancelling
                    | ChainStatus::Cancelled
            );
            audit(
                chain,
                ChainActor::Daemon,
                Uuid::new_v4(),
                "terminal evidence sealed",
            )?;
            if !held {
                if let Some(reason) = reason {
                    pause(chain, reason);
                } else {
                    advance(chain)?;
                }
            }
            let outcome_id = chain.request.outcome_id;
            if let Some(outcome) = state
                .outcomes
                .iter_mut()
                .find(|outcome| outcome.id == outcome_id)
            {
                outcome.last_activity_at = waku_protocol::model::unix_time();
            }
            Ok(())
        })
    }

    /// Deferred outcome closure for a completed finisher chain: the chain's
    /// own completion is already recorded, and the outcome closes only when
    /// its ordinary completion conditions are satisfied — otherwise it stays
    /// open and is rechecked the next time those conditions can change.
    /// Rechecking never reruns the chain's verification.
    pub fn settle_chain_finisher(&self, chain_id: Uuid) -> Result<Option<BossOutcome>> {
        let mut settled = None;
        self.update(|state| {
            let Some(chain) = state.chains.iter().find(|chain| chain.id == chain_id) else {
                return Ok(());
            };
            if chain.status != ChainStatus::Completed || !chain.finisher_reserved {
                return Ok(());
            }
            let outcome_id = chain.request.outcome_id;
            let Some(outcome) = state.outcomes.iter().find(|entry| entry.id == outcome_id) else {
                return Ok(());
            };
            if outcome.terminal() {
                settled = Some(outcome.clone());
                return Ok(());
            }
            let outstanding = outcome.completion_outstanding(
                &state.employees,
                &state.retired_employees,
                Uuid::nil(),
            );
            if !outstanding.is_empty() {
                settled = Some(outcome.clone());
                return Ok(());
            }
            let now = waku_protocol::model::unix_time();
            let evidence = format!(
                "Employee chain {chain_id} completed: {}",
                chain
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(index, step)| {
                        format!("step {} \"{}\" succeeded", index + 1, step.definition.title)
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            let outcome = state
                .outcomes
                .iter_mut()
                .find(|entry| entry.id == outcome_id)
                .ok_or_else(|| anyhow!("chain outcome is missing"))?;
            outcome.state = OutcomeState::Completed;
            outcome.completed_at = Some(now);
            outcome.completion_conflict = None;
            outcome.evidence = Some(evidence);
            outcome.last_activity_at = now;
            outcome.history.push(OutcomeTransition {
                state: OutcomeState::Completed,
                at: now,
                actor: PlanActor::Boss,
            });
            settled = Some(outcome.clone());
            Ok(())
        })?;
        Ok(settled)
    }
}

fn gate_reason(
    step: &ChainStep,
    result: &ChainResult,
    verification: &HandoffVerification,
    version: u64,
) -> Option<String> {
    if !verification.successor_access {
        return Some("successor input access is unconfirmed".into());
    }
    if result
        .checks
        .iter()
        .any(|check| check.status == CheckStatus::Fail)
    {
        return Some("a check failed".into());
    }
    for name in &step.definition.required_checks {
        if !result.checks.iter().any(|check| {
            &check.name == name
                && check.status == CheckStatus::Pass
                && !check.evidence.trim().is_empty()
        }) {
            return Some(format!("required check missing or unsuccessful: {name}"));
        }
    }
    for name in &step.definition.required_artifacts {
        if !result.artifacts.iter().any(|artifact| {
            &artifact.name == name
                && !artifact.reference.trim().is_empty()
                && verification
                    .accessible_artifacts
                    .contains(&artifact.reference)
        }) {
            return Some(format!("required artifact missing or inaccessible: {name}"));
        }
    }
    // Every supplied reference must be usable, including non-required evidence.
    if result.artifacts.iter().any(|artifact| {
        !verification
            .accessible_artifacts
            .contains(&artifact.reference)
    }) {
        return Some("artifact reference is inaccessible".into());
    }
    for gate in &step.definition.required_approvals {
        if !step
            .approvals
            .iter()
            .rev()
            .find(|approval| &approval.gate == gate && approval.definition_version == version)
            .is_some_and(|approval| approval.approved)
        {
            return Some(format!("approval required: {gate}"));
        }
    }
    if step.definition.boundary != HandoffBoundary::ArtifactsReady {
        let Some(code) = &result.code else {
            return Some("code evidence missing".into());
        };
        if verification.source_revision.as_ref() != Some(&code.source_revision) {
            return Some("source revision unconfirmed".into());
        }
        match step.definition.boundary {
            HandoffBoundary::CommittedWorkspace => {
                if !code.committed_revision.as_ref().is_some_and(|revision| {
                    verification.clean_committed_workspace.as_ref()
                        == Some(&(code.workspace.clone(), revision.clone()))
                }) {
                    return Some("clean committed workspace ownership unconfirmed".into());
                }
            }
            HandoffBoundary::Landed => {
                if !code
                    .landed_revision
                    .as_ref()
                    .zip(code.qa_branch.as_ref())
                    .zip(code.submission_id.as_ref())
                    .is_some_and(|((revision, branch), submission)| {
                        verification.confirmed_landing.as_ref()
                            == Some(&(revision.clone(), branch.clone(), submission.clone()))
                    })
                {
                    return Some("QA landing unconfirmed".into());
                }
            }
            HandoffBoundary::ArtifactsReady => {}
        }
    }
    None
}
fn advance(chain: &mut EmployeeChain) -> Result<()> {
    ensure!(
        chain.steps[chain.current_step].status == StepStatus::Succeeded,
        "handoff is not accepted"
    );
    if chain.current_step + 1 == chain.steps.len() {
        chain.status = ChainStatus::Completed;
        chain.reason = Some("chain complete; outcome closure remains separately gated".into());
    } else {
        chain.current_step += 1;
        // Advancement and the successor's durable admission key commit in the
        // same write — the host binds a real employee to this attempt's
        // `dispatch_id`, never a fabricated one.
        let step = &mut chain.steps[chain.current_step];
        step.status = StepStatus::Waiting;
        step.attempts.push(new_attempt(chain.definition_version));
        chain.status = ChainStatus::Waiting;
        chain.reason = Some(ADMISSION_PENDING.into());
    }
    Ok(())
}

/// Restart never invents admission or success. An uncertain bound attempt pauses
/// until the host reconciles its dispatch identity, runtime and workspace.
pub(crate) fn reconcile(state: &mut BossState) -> Result<()> {
    for chain in &mut state.chains {
        ensure!(
            chain.schema_version == SCHEMA_VERSION,
            "unsupported employee chain schema {}",
            chain.schema_version
        );
        ensure!(
            !chain.steps.is_empty() && chain.current_step < chain.steps.len(),
            "invalid chain step cursor"
        );
        if live(chain) {
            let attempt = chain.steps[chain.current_step].attempts.last();
            // A bound attempt only becomes uncertain once it could have
            // produced effects: starting or running work, a recorded
            // terminal or release, or a dead bound employee. One still
            // parked at the admission boundary — the durable ticket holds
            // the identity and the queue replays it — restarts clean.
            let uncertain = attempt.is_some_and(|attempt| {
                if attempt.sealed.is_some() {
                    return false;
                }
                attempt.terminal.is_some()
                    || attempt.execution_released
                    || attempt.workspace_released
                    || matches!(
                        attempt.status,
                        StepStatus::Starting | StepStatus::Working | StepStatus::CheckingHandoff
                    )
                    || attempt.employee_id.is_some_and(|employee| {
                        state
                            .employees
                            .iter()
                            .chain(state.retired_employees.iter())
                            .find(|entry| entry.session_id == employee)
                            .is_none_or(|entry| entry.expired)
                    })
            });
            if uncertain
                && !matches!(
                    chain.status,
                    ChainStatus::Pausing | ChainStatus::Cancelling | ChainStatus::Paused
                )
            {
                pause(
                    chain,
                    "restart requires execution and workspace reconciliation",
                );
                audit(
                    chain,
                    ChainActor::Daemon,
                    Uuid::new_v4(),
                    "restart reconciliation required",
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};
    use waku_protocol::{
        boss::{BossOperation, BossResult, EmployeeGoal, PersonaPermissions},
        model::ProviderKind,
    };

    struct Fixture {
        root: PathBuf,
        service: BossService,
        definition: ChainDefinition,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("chain-safety-{}", Uuid::new_v4()));
            let service = BossService::open(root.clone()).unwrap();
            let outcome_id = service
                .create_outcome(None, "Fix and verify", "checks pass")
                .unwrap()
                .id;
            let step = ChainStepDefinition {
                title: "Investigate".into(),
                prompt: "Investigate only".into(),
                project_id: Uuid::new_v4(),
                persona_id: None,
                provider: ProviderKind::Codex,
                model: "gpt-6.1-sol".into(),
                reasoning_effort: None,
                permissions: PersonaPermissions::default(),
                resources: Default::default(),
                workspace: AgentWorkspace::Local,
                base_revision: None,
                boundary: HandoffBoundary::ArtifactsReady,
                required_checks: vec!["repro".into()],
                required_artifacts: vec!["report".into()],
                required_approvals: vec![],
                max_retries: 1,
                plan_id: None,
                item_id: None,
                successor_brief: "Verify the report".into(),
            };
            let definition = ChainDefinition {
                outcome_id,
                finishes_outcome: false,
                steps: vec![step.clone(), step],
            };
            Self {
                root,
                service,
                definition,
            }
        }
        fn create(&self) -> EmployeeChain {
            self.service
                .chain_operation(
                    None,
                    ChainOperation::Create {
                        request_id: Uuid::new_v4(),
                        definition: self.definition.clone(),
                    },
                )
                .unwrap()
        }
        fn read(&self, id: Uuid) -> EmployeeChain {
            self.service
                .chain_operation(None, ChainOperation::Show { chain_id: id })
                .unwrap()
        }
        fn observe(&self, chain: &EmployeeChain, observation: ChainObservation) {
            let attempt = chain.steps[chain.current_step].attempts.last().unwrap();
            self.service
                .observe_chain(
                    chain.id,
                    attempt.id,
                    attempt.definition_version,
                    Uuid::new_v4(),
                    observation,
                )
                .unwrap();
        }
        fn admit(&self, chain: &EmployeeChain) {
            self.observe(
                chain,
                ChainObservation::Admitted {
                    employee_id: Uuid::new_v4(),
                },
            );
        }
        fn packet(&self, chain: &EmployeeChain, disposition: ResultDisposition) {
            let attempt = chain.steps[chain.current_step].attempts.last().unwrap();
            self.service
                .chain_operation(
                    None,
                    ChainOperation::RecordResult {
                        chain_id: chain.id,
                        attempt_id: attempt.id,
                        definition_version: attempt.definition_version,
                        result: packet(disposition),
                    },
                )
                .unwrap();
        }
        fn terminal(&self, chain: &EmployeeChain, disposition: ResultDisposition) {
            self.observe(
                chain,
                ChainObservation::Terminal {
                    terminal: ChainTerminal {
                        message: "Exact final\n  ```code```\n".into(),
                        disposition,
                    },
                },
            );
        }
        fn release(&self, chain: &EmployeeChain) {
            self.observe(
                chain,
                ChainObservation::Released {
                    execution: true,
                    workspace: true,
                },
            );
        }
        fn seal(&self, chain: &EmployeeChain, verification: &HandoffVerification) -> Result<()> {
            let attempt = chain.steps[chain.current_step].attempts.last().unwrap();
            self.service.seal_chain_handoff(
                chain.id,
                attempt.id,
                attempt.definition_version,
                verification,
            )
        }
        fn control(&self, id: Uuid, control: ChainControl) -> Result<EmployeeChain> {
            let revision = self.read(id).revision;
            self.service.chain_operation(
                None,
                ChainOperation::Control {
                    chain_id: id,
                    expected_revision: revision,
                    control,
                    source_revision: (control == ChainControl::Retry)
                        .then(|| "verified-source".into()),
                },
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn packet(disposition: ResultDisposition) -> ChainResult {
        ChainResult {
            disposition,
            checks: vec![ChainCheck {
                name: "repro".into(),
                status: CheckStatus::Pass,
                evidence: "ran controlled reproduction".into(),
            }],
            artifacts: vec![ChainArtifact {
                name: "report".into(),
                reference: "artifact:report".into(),
            }],
            code: None,
            limitations: vec![],
        }
    }
    fn verified() -> HandoffVerification {
        HandoffVerification {
            accessible_artifacts: vec!["artifact:report".into()],
            successor_access: true,
            ..Default::default()
        }
    }

    #[test]
    fn creation_is_atomic_idempotent_and_linear() {
        let fixture = Fixture::new();
        let id = Uuid::new_v4();
        let operation = ChainOperation::Create {
            request_id: id,
            definition: fixture.definition.clone(),
        };
        let chain = fixture
            .service
            .chain_operation(None, operation.clone())
            .unwrap();
        let revision = fixture.service.document().revision;
        assert_eq!(
            fixture.service.chain_operation(None, operation).unwrap().id,
            chain.id
        );
        assert_eq!(fixture.service.document().revision, revision);
        assert_eq!(chain.steps[0].attempts.len(), 1);
        assert_eq!(chain.steps[1].status, StepStatus::NotStarted);
        assert!(chain.steps[1].attempts.is_empty());
        assert_eq!(chain.status, ChainStatus::Waiting); // no fabricated queue admission
        let mut changed = fixture.definition.clone();
        changed.steps[1].prompt = "Other scope".into();
        assert!(
            fixture
                .service
                .chain_operation(
                    None,
                    ChainOperation::Create {
                        request_id: id,
                        definition: changed
                    }
                )
                .is_err()
        );
        let before = fixture.service.document();
        let mut invalid = fixture.definition.clone();
        invalid.steps[1].required_checks.push("repro".into());
        assert!(
            fixture
                .service
                .chain_operation(
                    None,
                    ChainOperation::Create {
                        request_id: Uuid::new_v4(),
                        definition: invalid
                    }
                )
                .is_err()
        );
        assert!(same(&before, &fixture.service.document()).unwrap());
        let reopened = BossService::open(fixture.root.clone()).unwrap();
        assert_eq!(
            reopened.document().chains[0].steps[0].attempts[0].dispatch_id,
            chain.steps[0].attempts[0].dispatch_id
        );
    }

    #[test]
    fn concurrent_duplicate_creation_commits_one_dispatch_identity() {
        let fixture = Fixture::new();
        let request_id = Uuid::new_v4();
        let ids = std::thread::scope(|scope| {
            let handles = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        fixture
                            .service
                            .chain_operation(
                                None,
                                ChainOperation::Create {
                                    request_id,
                                    definition: fixture.definition.clone(),
                                },
                            )
                            .unwrap()
                            .id
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(ids.iter().all(|id| *id == ids[0]));
        assert_eq!(fixture.service.document().chains.len(), 1);
        assert_eq!(
            fixture.service.document().chains[0].steps[0].attempts.len(),
            1
        );
    }

    #[test]
    fn restart_before_sealing_holds_success_until_explicit_resume() {
        let fixture = Fixture::new();
        let chain = fixture.create();
        fixture.admit(&chain);
        fixture.packet(&chain, ResultDisposition::Success);
        fixture.terminal(&chain, ResultDisposition::Success);
        fixture.release(&chain);
        let reopened = BossService::open(fixture.root.clone()).unwrap();
        let attempt = &chain.steps[0].attempts[0];
        assert_eq!(reopened.document().chains[0].status, ChainStatus::Paused);
        reopened
            .seal_chain_handoff(chain.id, attempt.id, 1, &verified())
            .unwrap();
        let current = reopened.document().chains[0].clone();
        assert_eq!(current.current_step, 0);
        assert_eq!(current.steps[0].status, StepStatus::Succeeded);
        let resumed = reopened
            .chain_operation(
                None,
                ChainOperation::Control {
                    chain_id: chain.id,
                    expected_revision: current.revision,
                    control: ChainControl::Resume,
                    source_revision: None,
                },
            )
            .unwrap();
        assert_eq!(resumed.current_step, 1);
        // Advancement persists the successor's admission key — the daemon
        // binds a real employee to it; a fabricated id cannot appear.
        assert_eq!(resumed.steps[1].attempts.len(), 1);
        assert!(resumed.steps[1].attempts[0].employee_id.is_none());
        assert_eq!(resumed.steps[1].status, StepStatus::Waiting);
        assert_eq!(resumed.status, ChainStatus::Waiting);
    }

    #[test]
    fn terminal_failure_overrides_early_success_and_release_is_mandatory() {
        let fixture = Fixture::new();
        let chain = fixture.create();
        fixture.admit(&chain);
        fixture.packet(&chain, ResultDisposition::Success);
        fixture.terminal(&chain, ResultDisposition::Failed);
        assert!(fixture.seal(&chain, &verified()).is_err());
        fixture.observe(
            &chain,
            ChainObservation::Released {
                execution: true,
                workspace: false,
            },
        );
        assert!(fixture.seal(&chain, &verified()).is_err());
        fixture.release(&chain);
        fixture.seal(&chain, &verified()).unwrap();
        let current = fixture.read(chain.id);
        assert_eq!(current.status, ChainStatus::Paused);
        assert_eq!(current.current_step, 0);
        let handoff = current.steps[0].attempts[0].sealed.as_ref().unwrap();
        assert_eq!(handoff.terminal_message, "Exact final\n  ```code```\n");
        assert_eq!(handoff.result.disposition, ResultDisposition::Failed);
        assert_eq!(
            current.steps[0].attempts[0]
                .provisional_result
                .as_ref()
                .unwrap()
                .disposition,
            ResultDisposition::Success
        );
        assert!(fixture.control(chain.id, ChainControl::Resume).is_err());
        assert!(fixture.control(chain.id, ChainControl::Retry).is_err());
        fixture.observe(
            &chain,
            ChainObservation::EffectsReconciled {
                source_revision: "verified-source".into(),
            },
        );
        let retry = fixture.control(chain.id, ChainControl::Retry).unwrap();
        assert_ne!(
            retry.steps[0].attempts[1].dispatch_id,
            chain.steps[0].attempts[0].dispatch_id
        );
        assert_eq!(retry.steps[0].attempts.len(), 2);
        assert!(
            fixture
                .service
                .observe_chain(
                    chain.id,
                    chain.steps[0].attempts[0].id,
                    1,
                    Uuid::new_v4(),
                    ChainObservation::Idle
                )
                .is_err()
        );
    }

    #[test]
    fn acceptance_and_successor_resolution_are_one_duplicate_safe_write() {
        let fixture = Fixture::new();
        let chain = fixture.create();
        fixture.admit(&chain);
        fixture.packet(&chain, ResultDisposition::Success);
        fixture.terminal(&chain, ResultDisposition::Success);
        fixture.release(&chain);
        fixture.seal(&chain, &verified()).unwrap();
        let accepted = fixture.read(chain.id);
        assert_eq!(accepted.current_step, 1);
        assert_eq!(accepted.steps[0].status, StepStatus::Succeeded);
        // Advancement commits the successor's admission key in the same
        // write — unbound until the daemon admits a real employee to it.
        assert_eq!(accepted.steps[1].attempts.len(), 1);
        assert!(accepted.steps[1].attempts[0].employee_id.is_none());
        assert_eq!(accepted.steps[1].status, StepStatus::Waiting);
        assert_eq!(
            accepted.steps[0].attempts[0]
                .sealed
                .as_ref()
                .unwrap()
                .successor_step,
            Some(1)
        );
        let revision = fixture.service.document().revision;
        fixture.seal(&chain, &verified()).unwrap();
        assert_eq!(fixture.service.document().revision, revision);
        let reopened = BossService::open(fixture.root.clone()).unwrap();
        assert_eq!(reopened.document().chains[0].current_step, 1);
        assert_eq!(reopened.document().chains[0].steps[1].attempts.len(), 1);
    }

    #[test]
    fn missing_access_pauses_once_and_repaired_gate_needs_explicit_resume() {
        let fixture = Fixture::new();
        let chain = fixture.create();
        fixture.admit(&chain);
        fixture.packet(&chain, ResultDisposition::Success);
        fixture.terminal(&chain, ResultDisposition::Success);
        fixture.release(&chain);
        fixture
            .seal(&chain, &HandoffVerification::default())
            .unwrap();
        let paused = fixture.read(chain.id);
        let attention = paused.attention.unwrap().id;
        assert_eq!(paused.status, ChainStatus::Paused);
        fixture.seal(&chain, &verified()).unwrap();
        assert_eq!(fixture.read(chain.id).attention.unwrap().id, attention);
        let attempt = &chain.steps[0].attempts[0];
        fixture
            .service
            .recheck_chain_handoff(chain.id, attempt.id, 1, &verified())
            .unwrap();
        assert_eq!(fixture.read(chain.id).current_step, 0);
        assert_eq!(
            fixture.read(chain.id).steps[0].status,
            StepStatus::Succeeded
        );
        let resumed = fixture.control(chain.id, ChainControl::Resume).unwrap();
        assert_eq!(resumed.current_step, 1);
        assert_eq!(resumed.attention.unwrap().id, attention);
    }

    #[test]
    fn pause_and_cancel_racing_finish_never_advance() {
        for control in [ChainControl::Pause, ChainControl::Cancel] {
            let fixture = Fixture::new();
            let chain = fixture.create();
            fixture.admit(&chain);
            fixture.packet(&chain, ResultDisposition::Success);
            let stopping = fixture.control(chain.id, control).unwrap();
            assert_eq!(
                stopping.status,
                if control == ChainControl::Pause {
                    ChainStatus::Pausing
                } else {
                    ChainStatus::Cancelling
                }
            );
            assert!(fixture.control(chain.id, ChainControl::Resume).is_err());
            fixture.terminal(&chain, ResultDisposition::Success);
            fixture.release(&chain);
            fixture.seal(&chain, &verified()).unwrap();
            let result = fixture.read(chain.id);
            assert_eq!(result.current_step, 0);
            assert_eq!(
                result.status,
                if control == ChainControl::Pause {
                    ChainStatus::Paused
                } else {
                    ChainStatus::Cancelled
                }
            );
            assert!(result.steps[1].attempts.is_empty());
            if control == ChainControl::Pause {
                assert_eq!(
                    fixture
                        .control(chain.id, ChainControl::Resume)
                        .unwrap()
                        .current_step,
                    1
                );
            } else {
                assert!(fixture.control(chain.id, ChainControl::Retry).is_err());
            }
        }
    }

    #[test]
    fn cancel_release_before_terminal_retains_late_evidence() {
        let fixture = Fixture::new();
        let chain = fixture.create();
        fixture.admit(&chain);
        fixture.packet(&chain, ResultDisposition::Success);
        fixture.control(chain.id, ChainControl::Cancel).unwrap();
        fixture.release(&chain);
        fixture.terminal(&chain, ResultDisposition::Success);
        fixture.seal(&chain, &verified()).unwrap();
        assert_eq!(fixture.read(chain.id).status, ChainStatus::Cancelled);
        assert!(fixture.read(chain.id).steps[0].attempts[0].sealed.is_some());
    }

    #[test]
    fn lifecycle_observations_are_truthful_stale_safe_and_restart_pauses_unknown_execution() {
        let fixture = Fixture::new();
        let chain = fixture.create();
        let attempt = &chain.steps[0].attempts[0];
        assert!(
            fixture
                .service
                .observe_chain(
                    chain.id,
                    attempt.id,
                    2,
                    Uuid::new_v4(),
                    ChainObservation::Starting
                )
                .is_err()
        );
        assert!(
            fixture
                .service
                .observe_chain(
                    chain.id,
                    attempt.id,
                    1,
                    Uuid::new_v4(),
                    ChainObservation::Starting
                )
                .is_err()
        );
        let event = Uuid::new_v4();
        let employee_id = Uuid::new_v4();
        fixture
            .service
            .observe_chain(
                chain.id,
                attempt.id,
                1,
                event,
                ChainObservation::Admitted { employee_id },
            )
            .unwrap();
        let revision = fixture.service.document().revision;
        fixture
            .service
            .observe_chain(
                chain.id,
                attempt.id,
                1,
                event,
                ChainObservation::Admitted { employee_id },
            )
            .unwrap();
        assert_eq!(fixture.service.document().revision, revision);
        assert_eq!(fixture.read(chain.id).status, ChainStatus::Queued);
        fixture.observe(&chain, ChainObservation::Starting);
        assert_eq!(fixture.read(chain.id).status, ChainStatus::Starting);
        fixture.observe(
            &chain,
            ChainObservation::Working {
                starting_revision: "source".into(),
                workspace: "workspace".into(),
            },
        );
        assert_eq!(fixture.read(chain.id).status, ChainStatus::Working);
        fixture.observe(&chain, ChainObservation::Idle);
        assert_eq!(
            fixture.read(chain.id).status,
            ChainStatus::WaitingForEmployee
        );
        let reopened = BossService::open(fixture.root.clone()).unwrap();
        let recovered = &reopened.document().chains[0];
        assert_eq!(recovered.status, ChainStatus::Paused);
        assert_eq!(
            recovered.steps[0].attempts[0].employee_id,
            Some(employee_id)
        );
        assert_eq!(
            recovered.steps[0].attempts[0].dispatch_id,
            attempt.dispatch_id
        );
        assert!(recovered.steps[0].attempts[0].sealed.is_none());
        assert!(
            reopened
                .chain_operation(
                    None,
                    ChainOperation::Control {
                        chain_id: chain.id,
                        expected_revision: recovered.revision,
                        control: ChainControl::Resume,
                        source_revision: None
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn launch_error_is_not_success_even_without_structured_result() {
        let fixture = Fixture::new();
        let chain = fixture.create();
        fixture.admit(&chain);
        fixture.observe(
            &chain,
            ChainObservation::LaunchError {
                reason: "provider launch failed".into(),
            },
        );
        fixture.release(&chain);
        fixture.seal(&chain, &verified()).unwrap();
        assert_eq!(fixture.read(chain.id).status, ChainStatus::Paused);
        assert_eq!(
            fixture.read(chain.id).steps[0].attempts[0]
                .sealed
                .as_ref()
                .unwrap()
                .terminal_message,
            "provider launch failed"
        );
    }

    #[test]
    fn revisions_and_approvals_do_not_invalidate_an_existing_attempt_or_authorize_rerun() {
        let mut fixture = Fixture::new();
        fixture.definition.steps[0]
            .required_approvals
            .push("human review".into());
        let chain = fixture.create();
        fixture.admit(&chain);
        fixture.control(chain.id, ChainControl::Pause).unwrap();
        fixture.packet(&chain, ResultDisposition::Success);
        fixture.terminal(&chain, ResultDisposition::Success);
        fixture.release(&chain);
        let paused = fixture.read(chain.id);
        let mut definition = fixture.definition.steps[1].clone();
        definition.prompt = "Use amended brief".into();
        let operation = ChainOperation::Revise {
            chain_id: chain.id,
            expected_revision: paused.revision,
            expected_definition_version: 1,
            step: 1,
            definition,
        };
        let revised = fixture
            .service
            .chain_operation(None, operation.clone())
            .unwrap();
        assert_eq!(revised.definition_version, 2);
        assert!(fixture.service.chain_operation(None, operation).is_err());
        fixture.seal(&chain, &verified()).unwrap();
        let failed_gate = fixture.read(chain.id);
        assert_eq!(failed_gate.current_step, 0);
        assert!(fixture.control(chain.id, ChainControl::Resume).is_err());
        let boss = Uuid::new_v4();
        fixture.service.set_session_id(boss).unwrap();
        assert!(
            fixture
                .service
                .chain_operation(
                    Some(boss),
                    ChainOperation::Approve {
                        chain_id: chain.id,
                        expected_revision: failed_gate.revision,
                        gate: "human review".into(),
                        approved: true,
                    }
                )
                .is_err()
        );
        fixture
            .service
            .chain_operation(
                None,
                ChainOperation::Approve {
                    chain_id: chain.id,
                    expected_revision: failed_gate.revision,
                    gate: "human review".into(),
                    approved: true,
                },
            )
            .unwrap();
        fixture
            .service
            .recheck_chain_handoff(chain.id, chain.steps[0].attempts[0].id, 1, &verified())
            .unwrap();
        let accepted = fixture.read(chain.id);
        fixture
            .service
            .chain_operation(
                None,
                ChainOperation::Approve {
                    chain_id: chain.id,
                    expected_revision: accepted.revision,
                    gate: "human review".into(),
                    approved: false,
                },
            )
            .unwrap();
        assert!(fixture.control(chain.id, ChainControl::Resume).is_err());
        let denied = fixture.read(chain.id);
        fixture
            .service
            .chain_operation(
                None,
                ChainOperation::Approve {
                    chain_id: chain.id,
                    expected_revision: denied.revision,
                    gate: "human review".into(),
                    approved: true,
                },
            )
            .unwrap();
        fixture
            .service
            .recheck_chain_handoff(chain.id, chain.steps[0].attempts[0].id, 1, &verified())
            .unwrap();
        assert_eq!(
            fixture
                .control(chain.id, ChainControl::Resume)
                .unwrap()
                .current_step,
            1
        );
    }

    #[test]
    fn finisher_reservation_conflicts_with_ordinary_admission_and_cancel_releases_it() {
        let mut fixture = Fixture::new();
        fixture.definition.finishes_outcome = true;
        let chain = fixture.create();
        assert!(
            fixture
                .service
                .chain_operation(
                    None,
                    ChainOperation::Create {
                        request_id: Uuid::new_v4(),
                        definition: fixture.definition.clone()
                    }
                )
                .is_err()
        );
        assert!(
            fixture
                .service
                .assignment_admission(
                    Some(chain.request.outcome_id),
                    None,
                    true,
                    None,
                    vec![],
                    None
                )
                .is_err()
        );
        fixture.control(chain.id, ChainControl::Cancel).unwrap();
        assert!(
            fixture
                .service
                .assignment_admission(
                    Some(chain.request.outcome_id),
                    None,
                    true,
                    None,
                    vec![],
                    None
                )
                .is_ok()
        );
    }

    #[test]
    fn employee_cannot_control_chains_or_see_other_assignments() {
        let fixture = Fixture::new();
        let boss = Uuid::new_v4();
        fixture.service.set_session_id(boss).unwrap();
        let employee = fixture
            .service
            .prepare_employee(boss, None, "job".into(), None, false, EmployeeGoal::Errand, None)
            .unwrap();
        let employee_id = employee.session_id;
        fixture.service.add_employee(employee).unwrap();
        let chain = fixture.create();
        assert!(
            fixture
                .service
                .chain_operation(
                    Some(employee_id),
                    ChainOperation::Show { chain_id: chain.id }
                )
                .is_err()
        );
        let BossResult::State { state } = fixture
            .service
            .handle(Some(employee_id), BossOperation::View)
            .unwrap()
        else {
            panic!()
        };
        assert!(state.chains.is_empty());
        let attempt = &chain.steps[0].attempts[0];
        assert!(
            fixture
                .service
                .chain_operation(
                    Some(employee_id),
                    ChainOperation::RecordResult {
                        chain_id: chain.id,
                        attempt_id: attempt.id,
                        definition_version: 1,
                        result: packet(ResultDisposition::Success)
                    }
                )
                .is_err()
        );
        fixture.observe(&chain, ChainObservation::Admitted { employee_id });
        assert!(
            fixture
                .service
                .chain_operation(
                    Some(employee_id),
                    ChainOperation::RecordResult {
                        chain_id: chain.id,
                        attempt_id: attempt.id,
                        definition_version: 1,
                        result: packet(ResultDisposition::Success)
                    }
                )
                .is_ok()
        );
    }

    #[test]
    fn unsupported_schema_and_failed_persistence_fail_closed() {
        let fixture = Fixture::new();
        let chain = fixture.create();
        let before = fixture.service.document();
        fs::remove_file(fixture.root.join("boss.json")).unwrap();
        fs::create_dir(fixture.root.join("boss.json")).unwrap();
        assert!(fixture.control(chain.id, ChainControl::Pause).is_err());
        assert!(same(&before, &fixture.service.document()).unwrap());
        fs::remove_dir(fixture.root.join("boss.json")).unwrap();
        let mut invalid = before;
        invalid.chains[0].schema_version = 99;
        fs::write(
            fixture.root.join("boss.json"),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        assert!(BossService::open(fixture.root.clone()).is_err());
    }

    #[test]
    fn landed_boundary_requires_host_confirmed_matching_evidence() {
        let fixture = Fixture::new();
        let mut step = fixture.definition.steps[0].clone();
        step.boundary = HandoffBoundary::Landed;
        let mut result = packet(ResultDisposition::Success);
        result.code = Some(ChainCodeEvidence {
            workspace: "workspace".into(),
            source_revision: "source".into(),
            committed_revision: Some("original".into()),
            landed_revision: Some("rebased".into()),
            qa_branch: Some("dev".into()),
            submission_id: Some("submission".into()),
        });
        let step = ChainStep {
            definition: step,
            status: StepStatus::CheckingHandoff,
            attempts: vec![],
            approvals: vec![],
        };
        assert!(gate_reason(&step, &result, &verified(), 1).is_some());
        let mut verification = verified();
        verification.source_revision = Some("source".into());
        verification.confirmed_landing =
            Some(("rebased".into(), "dev".into(), "submission".into()));
        assert!(gate_reason(&step, &result, &verification, 1).is_none());
        verification.confirmed_landing =
            Some(("original".into(), "dev".into(), "submission".into()));
        assert!(gate_reason(&step, &result, &verification, 1).is_some());
    }
}
