//! Sequential employee chains — the execution adapter.
//!
//! The reducer in `waku_boss::chains` owns every durable decision; this
//! adapter performs the host effects it cannot: admission through the
//! ordinary summon queue, lifecycle observations from real runtime events,
//! terminal sealing against host-verified facts, and quiet routing —
//! routine intermediate success never wakes the Boss, exceptions surface
//! once per incident, and a completed chain reports once.
//!
//! ## Seam — artifact access and workspace handoff
//!
//! `chain_handoff.rs` owns moving artifacts and workspaces between
//! attempts; this adapter consumes its three plugs:
//!
//! - `verify_chain_handoff` — the seal's `HandoffVerification` (artifact
//!   reachability, committed-workspace cleanliness, confirmed landings,
//!   successor access). Unverifiable inputs fail the gate and pause.
//! - `chain_successor_inputs` — the workspace (`Adopt` vs fresh
//!   `Worktree`), adoption target, base revision, and handoff brief a
//!   successor summon receives, re-verified at dispatch so drift pauses
//!   rather than launching on dead references.
//! - `validate_chain_projects` — create/revise-time project policy checks
//!   (wired in `boss_ops`).
//!
//! The attempt's durable record is what the seam reads back:
//! `employee_id` for the bound session owning the checkout, `workspace`
//! and `starting_revision` observed at `Working`, `sealed.result`'s
//! artifact and code evidence, and `dispatch_id` for admission identity.

use super::*;
use waku_boss::{BossService, chains::ChainObservation};
use waku_protocol::{
    boss::{BossEmployee, EmployeeSettle, ExpiryCause, PlanAssignmentKind, SummonTicket},
    chains::*,
};

/// Observation event tags — xored with the attempt's durable `dispatch_id`
/// into deterministic event ids, so a re-driven finish, a replayed drive
/// pass, or a post-restart recovery folds onto the reducer's idempotence
/// instead of appending duplicate history.
pub(super) const OBSERVE_ADMITTED: u128 = 0x01;
pub(super) const OBSERVE_STARTING: u128 = 0x02;
pub(super) const OBSERVE_WORKING: u128 = 0x03;
pub(super) const OBSERVE_IDLE: u128 = 0x04;
pub(super) const OBSERVE_LAUNCH_ERROR: u128 = 0x05;
pub(super) const OBSERVE_TERMINAL: u128 = 0x06;
pub(super) const OBSERVE_RELEASED: u128 = 0x07;
pub(super) const OBSERVE_RECONCILED: u128 = 0x08;

/// A session's chain binding: the live current-step attempt it may still
/// report against, or a stale binding a newer attempt superseded.
#[derive(Clone, Copy)]
pub(super) enum ChainBinding {
    Current {
        chain_id: Uuid,
        chain_status: ChainStatus,
        attempt_id: Uuid,
        definition_version: u64,
        dispatch_id: Uuid,
        status: StepStatus,
        terminal: bool,
    },
    Stale,
}

/// The chain binding a session carries, if any — resolved from durable
/// state so every caller reconciles the same facts.
pub(super) fn chain_binding(boss: &BossService, session_id: Uuid) -> Option<ChainBinding> {
    let document = boss.document();
    chain_binding_in(&document, session_id)
}

/// Reuse a caller's snapshot when resolving multiple employee bindings.
pub(super) fn chain_binding_in(
    document: &waku_protocol::boss::BossState,
    session_id: Uuid,
) -> Option<ChainBinding> {
    let mut stale = false;
    for chain in &document.chains {
        if let Some(attempt) = chain.steps[chain.current_step].attempts.last()
            && attempt.employee_id == Some(session_id)
        {
            return Some(ChainBinding::Current {
                chain_id: chain.id,
                chain_status: chain.status,
                attempt_id: attempt.id,
                definition_version: attempt.definition_version,
                dispatch_id: attempt.dispatch_id,
                status: attempt.status,
                terminal: attempt.terminal.is_some(),
            });
        }
        stale |= chain
            .steps
            .iter()
            .flat_map(|step| step.attempts.iter())
            .any(|attempt| attempt.employee_id == Some(session_id));
    }
    stale.then_some(ChainBinding::Stale)
}

fn chain_event_id(dispatch_id: Uuid, tag: u128) -> Uuid {
    Uuid::from_u128(dispatch_id.as_u128() ^ tag)
}

pub(super) fn observe_quiet(
    boss: &BossService,
    chain_id: Uuid,
    attempt_id: Uuid,
    definition_version: u64,
    dispatch_id: Uuid,
    tag: u128,
    observation: ChainObservation,
) -> anyhow::Result<()> {
    boss.observe_chain(
        chain_id,
        attempt_id,
        definition_version,
        chain_event_id(dispatch_id, tag),
        observation,
    )
    .map_err(|error| {
        eprintln!("chain observation rejected for attempt {attempt_id}: {error:#}");
        error
    })
}

/// An admitted attempt whose session stopped making progress without a
/// terminal — a permission wait or an unanswered ask. Waiting is a status,
/// not a verdict: the terminal that follows still seals the attempt.
pub(super) fn observe_chain_idle(boss: &BossService, session_id: Uuid) {
    let Some(ChainBinding::Current {
        chain_id,
        attempt_id,
        definition_version,
        dispatch_id,
        status,
        ..
    }) = chain_binding(boss, session_id)
    else {
        return;
    };
    if status != StepStatus::Working {
        return;
    }
    let _ = observe_quiet(
        boss,
        chain_id,
        attempt_id,
        definition_version,
        dispatch_id,
        OBSERVE_IDLE,
        ChainObservation::Idle,
    );
}

/// The disposition a settled chain attempt reports — the reducer decides
/// what it means; this only classifies the employee's own settle record.
fn chain_disposition(employee: &BossEmployee) -> ResultDisposition {
    let cause = employee.expiry.as_ref().map(|expiry| expiry.cause);
    match cause {
        Some(ExpiryCause::Finished) if employee.blocker.is_some() => ResultDisposition::Blocked,
        Some(ExpiryCause::Finished) => ResultDisposition::Success,
        Some(ExpiryCause::UnansweredAsk) => ResultDisposition::NeedsGuidance,
        _ => ResultDisposition::Failed,
    }
}

fn git_output(path: &Path, args: &[&str]) -> Option<std::process::Output> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()
}

fn git_head(workspace: &Path) -> Option<String> {
    let output = git_output(workspace, &["rev-parse", "--verify", "HEAD"])?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|head| !head.is_empty())
}

impl WakuBackend {
    /// The chain binding a session carries — see the free function for the
    /// resolver; daemon call sites take it through `self`.
    pub(super) fn chain_binding(&self, session_id: Uuid) -> Option<ChainBinding> {
        chain_binding(&self.boss, session_id)
    }

    /// Resolve only a chain's accepted UUID, including before its shell is bound.
    /// Ordinary summons retain their existing path/registration behavior.
    pub(super) fn chain_employee_project(
        &self,
        employee: &waku_protocol::boss::BossEmployee,
    ) -> anyhow::Result<Option<(Uuid, PathBuf)>> {
        let document = self.boss.document();
        let project_id = document.chains.iter().find_map(|chain| {
            chain.steps.iter().find_map(|step| {
                step.attempts
                    .iter()
                    .any(|attempt| {
                        attempt.employee_id == Some(employee.session_id)
                            || employee.request_id == Some(attempt.dispatch_id)
                    })
                    .then_some(step.definition.project_id)
            })
        });
        let Some(project_id) = project_id else {
            return Ok(None);
        };
        let project = self.task_state.lock().projects.iter()
            .find(|project| project.id == project_id).cloned()
            .ok_or_else(|| anyhow!("project_unavailable: accepted project {project_id} is no longer registered; restore that registration or explicitly revise the chain; no replacement was selected"))?;
        let root = dunce::canonicalize(&project.path).with_context(|| format!(
            "project_unavailable: accepted project '{}' (ID {}) root '{}' is unavailable; restore its directory before retrying",
            project.name, project.id, project.path.display()))?;
        anyhow::ensure!(
            root.is_dir(),
            "project_unavailable: accepted project '{}' (ID {}) root is not a directory",
            project.name,
            project.id
        );
        Ok(Some((project_id, root)))
    }

    /// The workspace and revision a fresh dispatch actually observed — the
    /// attempt's recorded `startingRevision` a later verifier checks
    /// against, never the revision the summon merely intended.
    pub(super) fn chain_workspace_revision(
        &self,
        session_id: Uuid,
        ticket: &waku_protocol::boss::SummonTicket,
    ) -> (Option<String>, Option<String>) {
        let workspace = {
            let state = self.task_state.lock();
            state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| {
                    session.workspace.path().map(Path::to_path_buf).or_else(|| {
                        // A local checkout reports the project root —
                        // `workspace.path()` names only managed worktrees.
                        state
                            .projects
                            .iter()
                            .find(|project| project.id == session.project_id)
                            .map(|project| project.path.clone())
                    })
                })
        };
        let Some(workspace) = workspace else {
            return (None, None);
        };
        let revision = git_head(&workspace).or_else(|| ticket.base_branch.clone());
        (Some(workspace.display().to_string()), revision)
    }

    /// One idempotent pass over the durable chain set: admit what is
    /// eligible, stop what a control halted, finish a seal a restart
    /// interrupted, settle a finished chain's outcome reservation, and
    /// deliver the quiet routing owed — one exception per incident, one
    /// report per completed chain. Safe to call from any boss-affecting
    /// path; every action reconciles against stored state.
    pub(super) fn drive_chains(&self) {
        if !self.boss.is_active() {
            return;
        }
        let events = self.event_source.lock().clone();
        for chain in self.boss.document().chains {
            match chain.status {
                ChainStatus::Waiting => self.admit_chain_step(&chain, &events),
                ChainStatus::Pausing | ChainStatus::Cancelling => self.stop_chain_attempt(&chain),
                ChainStatus::Paused => {
                    self.finish_interrupted_seal(&chain);
                    let current = self
                        .boss
                        .document()
                        .chains
                        .into_iter()
                        .find(|entry| entry.id == chain.id)
                        .unwrap_or(chain);
                    self.deliver_chain_exception(&current, &events);
                }
                ChainStatus::Completed => {
                    if chain.finisher_reserved
                        && let Err(error) = self.boss.settle_chain_finisher(chain.id)
                    {
                        eprintln!("could not settle chain finisher {}: {error:#}", chain.id);
                    }
                    self.deliver_chain_final(&chain, &events);
                }
                _ => {}
            }
        }
    }

    /// Admit a waiting step through the ordinary summon machinery — the
    /// ticket queues for capacity like any other admission, and its stable
    /// dispatch id is the summon request id, so a crash between ticket and
    /// binding replays into the same employee instead of duplicating work.
    fn admit_chain_step(&self, chain: &EmployeeChain, events: &EventSink) {
        let step_index = chain.current_step;
        let step = &chain.steps[step_index];
        let Some(attempt) = step.attempts.last() else {
            return;
        };
        if attempt.employee_id.is_some() || attempt.sealed.is_some() {
            return;
        }
        // An unresolved Employee base is a human choice still owed — a wait,
        // not a launch failure. The next pass retries once it resolves.
        if !self.boss.employee_base_resolved() {
            return;
        }
        // A supervisor id captured at Create can outlive its authority —
        // boss rotation swaps the session, retirement drops the employee.
        // A stale stored id must never strand a chain: fall back to the
        // live boss session when the recorded supervisor can no longer
        // summon this step's persona.
        let Some(supervisor) = chain
            .supervisor_id
            .filter(|supervisor| {
                self.boss
                    .can_summon_employees(*supervisor, step.definition.persona_id)
            })
            .or_else(|| self.boss.document().session_id)
        else {
            return;
        };
        let definition = &step.definition;
        let Some(project) = self
            .task_state
            .lock()
            .projects
            .iter()
            .find(|project| project.id == definition.project_id)
            .cloned()
        else {
            return self.chain_admission_failed(chain, attempt, format!(
                "project_unavailable: accepted project {} is no longer registered; restore that registration or explicitly revise the chain; no replacement was selected", definition.project_id));
        };
        if !project.path.is_dir() {
            return self.chain_admission_failed(chain, attempt, format!(
                "project_unavailable: accepted project '{}' (ID {}) root '{}' is unavailable; restore its directory before retrying", project.name, project.id, project.path.display()));
        }
        // Handoff inputs are re-verified at dispatch: a drifted landing or
        // workspace surfaces as an error here, which pauses the chain —
        // never a summon on dead references.
        let inputs = match self.chain_successor_inputs(chain.id) {
            Ok(inputs) => inputs,
            Err(error) => {
                return self.chain_admission_failed(chain, attempt, format!("{error:#}"));
            }
        };
        let prompt = self.compose_chain_prompt(chain, step_index, attempt, inputs.brief.as_deref());
        let fingerprint = serde_json::to_string(&serde_json::json!({
            "chainId": chain.id,
            "attemptId": attempt.id,
            "definitionVersion": attempt.definition_version,
            "prompt": prompt,
            "step": definition,
            "workspace": inputs.workspace,
            "adoptWorktree": inputs.adopt_worktree,
            // The declared source, not the resolved commit — a ref that
            // moved since the ticket was queued must still bind the
            // already-admitted employee on replay.
            "baseRevision": &definition.base_revision,
        }));
        let Ok(fingerprint) = fingerprint else {
            return self.chain_admission_failed(chain, attempt, "chain step is not serializable");
        };
        // Replay safety: an employee already admitted under this attempt's
        // dispatch id binds rather than duplicating — a lost observe_chain
        // response or a crash between ticket and binding lands here.
        let document = self.boss.document();
        if let Some(existing) = document
            .employees
            .iter()
            .find(|employee| employee.request_id == Some(attempt.dispatch_id))
        {
            if existing.request_fingerprint.as_deref() == Some(fingerprint.as_str()) {
                let _ = observe_quiet(
                    &self.boss,
                    chain.id,
                    attempt.id,
                    attempt.definition_version,
                    attempt.dispatch_id,
                    OBSERVE_ADMITTED,
                    ChainObservation::Admitted {
                        employee_id: existing.session_id,
                    },
                );
                self.wake_summon_queue();
            } else {
                self.chain_admission_failed(
                    chain,
                    attempt,
                    "dispatch id collides with a different admission",
                );
            }
            return;
        }
        let result = (|| {
            let mut employee = self.boss.prepare_employee(
                supervisor,
                definition.persona_id,
                definition.title.clone(),
                None,
                false,
                waku_protocol::boss::EmployeeGoal::Errand,
                None,
            )?;
            // The step's declared permissions are the frozen grant — they
            // were validated at chain acceptance and never renegotiated.
            employee.permissions = definition.permissions.clone();
            employee.request_id = Some(attempt.dispatch_id);
            employee.request_fingerprint = Some(fingerprint);
            employee.assignment_kind = PlanAssignmentKind::Implementation;
            employee.purpose = definition.purpose;
            employee.plan_id = definition.plan_id;
            employee.item_id = definition.item_id;
            employee.plan_binding = self.chain_plan_binding(chain, definition)?;
            let resolved = self.resolve_agent_task_selection(
                Some(supervisor),
                &AgentCreateSelection {
                    provider: Some(definition.provider),
                    model: Some(definition.model.clone()),
                    reasoning_effort: definition.reasoning_effort.clone(),
                    title: Some(employee.identity.name.clone()),
                    service_tier: None,
                    context_window: None,
                },
                &project.path,
                &prompt,
                true,
            )?;
            let model_id = resolved
                .model
                .clone()
                .or_else(|| resolved.concrete_model.clone())
                .ok_or_else(|| anyhow!("chain step model did not resolve"))?;
            if !resource_set_empty(&definition.resources) {
                self.resource_broker()?
                    .validate_set(&definition.resources)?;
            }
            // Workspace and base selection come from the handoff seam:
            // adopt for a verified committed workspace, a fresh worktree
            // at the landed or reconciled revision, else the step's own.
            let ticket = SummonTicket {
                sequence: 0,
                generation: 1,
                provider: resolved.provider,
                model: model_id,
                reasoning_effort: resolved.reasoning_effort.clone(),
                prompt,
                project: project.path.display().to_string(),
                workspace: Some(inputs.workspace),
                base_branch: inputs.base_revision.clone(),
                adopt_worktree: inputs.adopt_worktree.clone(),
                resources: definition.resources.clone(),
                allow_burst: false,
                human_interaction: false,
                force_start: false,
                composer_receipts: Vec::new(),
                pending_prompts: Vec::new(),
                group_id: None,
                priority: None,
                outcome_id: None,
                reservation: None,
                pending_resources: None,
                pending_reservation: None,
                blocked_by: Vec::new(),
                dispatch_event: None,
                interruptions: Vec::new(),
                resume_count: 0,
                last_resumed_cause: None,
            };
            self.create_employee_shell(&employee, &resolved, &ticket)?;
            let session_id = employee.session_id;
            if let Err(error) = self.boss.enqueue_ticket(employee.clone(), ticket) {
                self.remove_session_shell(session_id);
                return Err(error);
            }
            if let Err(error) = self.post_summon_marker(supervisor, &employee, events) {
                eprintln!("could not record chain summon marker: {error:#}");
            }
            Ok::<_, anyhow::Error>(session_id)
        })();
        let session_id = match result {
            Ok(session_id) => session_id,
            Err(error) => {
                return self.chain_admission_failed(chain, attempt, format!("{error:#}"));
            }
        };
        // Admission binding commits after the ticket exists — a launch that
        // raced the queue between enqueue and this write gets stopped, never
        // detached from its attempt.
        if let Err(error) = observe_quiet(
            &self.boss,
            chain.id,
            attempt.id,
            attempt.definition_version,
            attempt.dispatch_id,
            OBSERVE_ADMITTED,
            ChainObservation::Admitted {
                employee_id: session_id,
            },
        ) {
            let _ = error;
            let _ = self.boss.mark_cancelled(session_id);
            let _ = self.finish_boss_employee(
                session_id,
                false,
                waku_protocol::boss::EmployeeSettle::Stopped,
            );
            return;
        }
        self.wake_summon_queue();
    }

    /// The assignment text a step's employee receives — its own prompt,
    /// the handoff brief the seam composed from the predecessor's sealed
    /// evidence, and the identities its structured-result write needs.
    fn compose_chain_prompt(
        &self,
        chain: &EmployeeChain,
        step_index: usize,
        attempt: &ChainAttempt,
        brief: Option<&str>,
    ) -> String {
        let step = &chain.steps[step_index];
        let mut prompt = step.definition.prompt.clone();
        prompt.push_str(&format!(
            "\n\n## Sequential chain step\n\nYou are step {} of {} in employee chain {}. Attempt {} at definition version {}.\n",
            step_index + 1,
            chain.steps.len(),
            chain.id,
            attempt.id,
            attempt.definition_version,
        ));
        if let Some(brief) = brief {
            prompt.push_str(&format!("\n{brief}\n"));
        }
        // The seal matches check/artifact names verbatim — hand the employee
        // the exact strings so a shortened report name can't false-fail a gate.
        if !step.definition.required_checks.is_empty() {
            prompt.push_str(&format!(
                "\nRequired checks — report each under this EXACT name (status + evidence):\n{}\n",
                step.definition
                    .required_checks
                    .iter()
                    .map(|name| format!("- {name}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !step.definition.required_artifacts.is_empty() {
            prompt.push_str(&format!(
                "\nRequired artifacts — report each under this EXACT name (reference):\n{}\n",
                step.definition
                    .required_artifacts
                    .iter()
                    .map(|name| format!("- {name}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !step.definition.required_approvals.is_empty() {
            prompt.push_str(&format!(
                "\nThis step requires human approval for: {} — state it in your final message.\n",
                step.definition.required_approvals.join(", ")
            ));
        }
        prompt.push_str(&format!(
            "\nRecord your structured result before finishing:\n\n```\ngoddard-agent boss chain --json '{{\"type\":\"recordResult\",\"chainId\":\"{}\",\"attemptId\":\"{}\",\"definitionVersion\":{},\"result\":{{\"disposition\":\"success|blocked|needsGuidance|failed\",\"checks\":[{{\"name\":\"...\",\"status\":\"pass|fail|notRun\",\"evidence\":\"...\"}}],\"artifacts\":[{{\"name\":\"...\",\"reference\":\"commit:<full sha>\"}}],\"code\":{{\"workspace\":\"...\",\"sourceRevision\":\"...\",\"committedRevision\":null,\"landedRevision\":null,\"qaBranch\":null,\"submissionId\":null}},\"limitations\":[]}}}}'\n```\n\nYour terminal message is sealed verbatim as this step's handoff to the next employee.",
            chain.id, attempt.id, attempt.definition_version,
        ));
        prompt
    }

    /// The plan tag a step declared, resolved like an ordinary summon —
    /// explicit plan by id, else the outcome's attached plan, else none.
    fn chain_plan_binding(
        &self,
        chain: &EmployeeChain,
        definition: &ChainStepDefinition,
    ) -> anyhow::Result<Option<waku_protocol::boss::PlanBinding>> {
        let explicit = definition
            .plan_id
            .map(|plan_id| {
                self.boss
                    .document()
                    .planning
                    .iter()
                    .find(|plan| plan.id == plan_id)
                    .map(|plan| plan.plan_file.clone())
                    .ok_or_else(|| anyhow!("chain plan is unavailable"))
            })
            .transpose()?;
        self.boss
            .effective_plan_binding(
                explicit.as_deref(),
                Some(chain.request.outcome_id),
                definition.item_id,
                PlanAssignmentKind::Implementation,
            )
            .map_err(|error| anyhow!("{error:#}"))
    }

    /// A step the daemon cannot even admit — the reducer records a failed
    /// terminal on the unbound attempt, both releases land, and the seal
    /// pauses the chain with the configuration problem as its attention.
    fn chain_admission_failed(
        &self,
        chain: &EmployeeChain,
        attempt: &ChainAttempt,
        reason: impl Into<String>,
    ) {
        let reason = reason.into();
        let observations = [
            (
                OBSERVE_LAUNCH_ERROR,
                ChainObservation::LaunchError {
                    reason: reason.clone(),
                },
            ),
            (
                OBSERVE_RELEASED,
                ChainObservation::Released {
                    execution: true,
                    workspace: true,
                },
            ),
        ];
        for (tag, observation) in observations {
            if observe_quiet(
                &self.boss,
                chain.id,
                attempt.id,
                attempt.definition_version,
                attempt.dispatch_id,
                tag,
                observation,
            )
            .is_err()
            {
                return;
            }
        }
        self.seal_chain_attempt(
            chain.id,
            attempt.id,
            attempt.definition_version,
            attempt.dispatch_id,
        );
    }

    /// A `Resume` on a gate-paused chain: re-verify the sealed handoff
    /// against live host facts before the control reaches the reducer.
    /// Repaired access, files, or landings accept the handoff and the
    /// `Resume` then advances; a gate that still fails leaves the chain
    /// paused with its updated reason — `true` reports that outcome and
    /// the caller skips the control, which demands a retry for sealed
    /// attempts.
    pub(super) fn chain_resume_recheck(&self, chain_id: Uuid) -> anyhow::Result<bool> {
        let gate_paused =
            self.boss
                .document()
                .chains
                .iter()
                .find(|chain| chain.id == chain_id)
                .filter(|chain| chain.status == ChainStatus::Paused)
                .and_then(|chain| chain.steps[chain.current_step].attempts.last())
                .filter(|attempt| {
                    attempt.sealed.is_some()
                        && !attempt.handoff_accepted
                        && attempt.terminal.as_ref().is_some_and(|terminal| {
                            terminal.disposition == ResultDisposition::Success
                        })
                })
                .map(|attempt| (attempt.id, attempt.definition_version));
        let Some((attempt_id, version)) = gate_paused else {
            return Ok(false);
        };
        let verification = self.verify_chain_handoff(chain_id, attempt_id, version)?;
        self.boss
            .recheck_chain_handoff(chain_id, attempt_id, version, &verification)?;
        Ok(!self
            .boss
            .document()
            .chains
            .iter()
            .find(|chain| chain.id == chain_id)
            .and_then(|chain| chain.steps[chain.current_step].attempts.last())
            .is_some_and(|attempt| attempt.handoff_accepted))
    }

    /// A Pause or Cancel racing a live attempt: the reducer already marked
    /// the chain stopping; the attempt stays unreleased until its employee
    /// actually dies — this is that stop. Evidence the race still produces
    /// seals and stays retained; advancement cannot happen either way.
    fn stop_chain_attempt(&self, chain: &EmployeeChain) {
        let Some(attempt) = chain.steps[chain.current_step].attempts.last() else {
            return;
        };
        if attempt.execution_released && attempt.workspace_released {
            return;
        }
        let Some(employee_id) = attempt.employee_id else {
            return;
        };
        let _ = self.boss.mark_cancelled(employee_id);
        if let Err(error) = self.finish_boss_employee(
            employee_id,
            false,
            waku_protocol::boss::EmployeeSettle::Stopped,
        ) {
            eprintln!("could not stop chain attempt employee {employee_id}: {error:#}");
        }
    }

    /// A restart can land between a released attempt's terminal write and
    /// its seal — reconcile paused on the uncertain binding; complete the
    /// deferred seal and retry reconciliation when durable facts prove both
    /// releases. An attempt still missing its terminal or a release stays
    /// paused for the owner.
    fn finish_interrupted_seal(&self, chain: &EmployeeChain) {
        let Some(attempt) = chain.steps[chain.current_step].attempts.last() else {
            return;
        };
        if attempt.terminal.is_none() || !(attempt.execution_released && attempt.workspace_released)
        {
            return;
        }
        self.seal_chain_attempt(
            chain.id,
            attempt.id,
            attempt.definition_version,
            attempt.dispatch_id,
        );
    }

    /// The chain half of an employee's expiry: observe the terminal,
    /// confirm both releases, seal the handoff against host facts, then
    /// drive whatever the seal produced — quiet advancement, one incident
    /// exception, or the chain's single final report. Returns `true` when
    /// the session was chain-bound and ordinary reporting must stay off.
    pub(super) fn finish_chain_attempt(
        &self,
        employee: &BossEmployee,
        settle: EmployeeSettle,
        failed: bool,
        final_message: &str,
    ) -> anyhow::Result<bool> {
        let session_id = employee.session_id;
        let Some(binding) = chain_binding(&self.boss, session_id) else {
            return Ok(false);
        };
        let ChainBinding::Current {
            chain_id,
            attempt_id,
            definition_version,
            dispatch_id,
            terminal,
            ..
        } = binding
        else {
            // A superseded attempt's employee finishing late — its own
            // sealed record stands; the chain owns the silence either way.
            return Ok(true);
        };
        if matches!(settle, EmployeeSettle::LaunchFailed) {
            let reason = employee
                .blocker
                .clone()
                .unwrap_or_else(|| "employee launch failed".into());
            let _ = observe_quiet(
                &self.boss,
                chain_id,
                attempt_id,
                definition_version,
                dispatch_id,
                OBSERVE_LAUNCH_ERROR,
                ChainObservation::LaunchError { reason },
            );
        } else if !terminal {
            let _ = observe_quiet(
                &self.boss,
                chain_id,
                attempt_id,
                definition_version,
                dispatch_id,
                OBSERVE_TERMINAL,
                ChainObservation::Terminal {
                    terminal: ChainTerminal {
                        message: final_message.to_owned(),
                        disposition: chain_disposition(employee),
                    },
                },
            );
        }
        let _ = failed;
        let _ = observe_quiet(
            &self.boss,
            chain_id,
            attempt_id,
            definition_version,
            dispatch_id,
            OBSERVE_RELEASED,
            ChainObservation::Released {
                execution: true,
                workspace: true,
            },
        );
        self.seal_chain_attempt(chain_id, attempt_id, definition_version, dispatch_id);
        self.drive_chains();
        Ok(true)
    }

    /// Seal one attempt with host-checked facts. The reducer decides
    /// acceptance; failures only ever leave the attempt unsealed — a
    /// retried or recovered pass reconciles the same facts again.
    fn seal_chain_attempt(
        &self,
        chain_id: Uuid,
        attempt_id: Uuid,
        version: u64,
        dispatch_id: Uuid,
    ) {
        let Some(chain) = self
            .boss
            .document()
            .chains
            .into_iter()
            .find(|chain| chain.id == chain_id)
        else {
            return;
        };
        let Some(attempt) = chain.steps[chain.current_step].attempts.last() else {
            return;
        };
        if attempt.id != attempt_id {
            return;
        }
        // The handoff seam performs the real filesystem and Git checks —
        // unverifiable inputs fail the gate and pause rather than widen
        // access.
        if attempt.sealed.is_none() {
            let verification = match self.verify_chain_handoff(chain_id, attempt_id, version) {
                Ok(verification) => verification,
                Err(error) => {
                    eprintln!("chain handoff verification failed for {chain_id}: {error:#}");
                    return;
                }
            };
            if let Err(error) =
                self.boss
                    .seal_chain_handoff(chain_id, attempt_id, version, &verification)
            {
                eprintln!("chain handoff seal rejected for {chain_id}: {error:#}");
                return;
            }
        }
        // Record the fresh-workspace retry base after an unsuccessful seal.
        // Preserve the attempt's base when effects may exist; only an
        // unbound attempt can fall back to the project's verified HEAD.
        let Some(chain) = self
            .boss
            .document()
            .chains
            .into_iter()
            .find(|chain| chain.id == chain_id)
        else {
            return;
        };
        let step = &chain.steps[chain.current_step];
        if let Some(attempt) = step.attempts.last()
            && attempt.id == attempt_id
            && attempt.sealed.is_some()
            && !attempt.handoff_accepted
            && attempt.reconciled_retry_revision.is_none()
            && let Some(revision) = attempt
                .starting_revision
                .clone()
                .or_else(|| {
                    // The declared base resolved to a commit — refs and
                    // kinded sources never reach the durable record.
                    self.chain_step_base(&chain, chain.current_step)
                        .ok()
                        .flatten()
                })
                .or_else(|| {
                    // An unbound launch failure has no workspace effects.
                    // Successors need not declare a base: verify the project
                    // HEAD before recording a fresh-worktree retry source.
                    if attempt.employee_id.is_some() || attempt.workspace.is_some() {
                        return None;
                    }
                    let project_path = self
                        .task_state
                        .lock()
                        .projects
                        .iter()
                        .find(|project| project.id == step.definition.project_id)
                        .map(|project| project.path.clone())?;
                    git_head(&project_path)
                })
        {
            let _ = observe_quiet(
                &self.boss,
                chain_id,
                attempt_id,
                version,
                dispatch_id,
                OBSERVE_RECONCILED,
                ChainObservation::EffectsReconciled {
                    source_revision: revision,
                },
            );
        }
    }

    /// The supervisor's durable report for an unhandled chain attention —
    /// one per incident, keyed by the attention id so redelivery after a
    /// restart folds onto the same message instead of stacking.
    fn deliver_chain_exception(&self, chain: &EmployeeChain, events: &EventSink) {
        let Some(attention) = chain
            .attention
            .as_ref()
            .filter(|attention| attention.handled_at.is_none())
        else {
            return;
        };
        let Some(target) = self.chain_report_target(chain) else {
            return;
        };
        if self.boss_lineage_message_landed(target, attention.id) {
            return;
        }
        let step = &chain.steps[chain.current_step];
        let attempt = step.attempts.last();
        let sender = attempt.and_then(|attempt| attempt.employee_id);
        let kind = attempt
            .and_then(|attempt| attempt.terminal.as_ref())
            .map_or(
                crate::model::ReportTriggerKind::Failed,
                |terminal| match terminal.disposition {
                    ResultDisposition::Blocked => {
                        crate::model::ReportTriggerKind::FinishedWithBlocker
                    }
                    ResultDisposition::NeedsGuidance => crate::model::ReportTriggerKind::Blocker,
                    _ => crate::model::ReportTriggerKind::Failed,
                },
            );
        let prompt = format!(
            "Employee chain {} paused at step {} \"{}\" — {}. Work and evidence are preserved. Inspect with goddard-agent boss chain --json '{{\"type\":\"show\",\"chainId\":\"{}\"}}', then resume, retry, or cancel through the chain controls.",
            chain.id,
            chain.current_step + 1,
            step.definition.title,
            attention.reason,
            chain.id,
        );
        let trigger = sender
            .and_then(|id| self.boss.employee_including_retired(id))
            .map(|employee| crate::model::ReportTrigger::new(&employee, kind));
        if let Err(error) = self.queue_agent_prompt_with_id(
            target,
            prompt,
            sender,
            true,
            Some(attention.id),
            trigger,
            events,
        ) {
            eprintln!(
                "could not deliver chain exception {}: {error:#}",
                attention.id
            );
        }
    }

    /// The chain's single consolidated report: every accepted step's
    /// disposition and checks, the final terminal message verbatim, and
    /// the outcome's actual state — including a finisher reservation that
    /// could not close it yet.
    fn deliver_chain_final(&self, chain: &EmployeeChain, events: &EventSink) {
        let Some(target) = self.chain_report_target(chain) else {
            return;
        };
        if self.boss_lineage_message_landed(target, chain.id) {
            return;
        }
        let document = self.boss.document();
        let mut lines = vec![format!(
            "Employee chain {} finished — all {} step{} accepted.",
            chain.id,
            chain.steps.len(),
            if chain.steps.len() == 1 { "" } else { "s" },
        )];
        for (index, step) in chain.steps.iter().enumerate() {
            let Some(handoff) = step
                .attempts
                .last()
                .and_then(|attempt| attempt.sealed.as_ref())
            else {
                continue;
            };
            let passed = handoff
                .result
                .checks
                .iter()
                .filter(|check| check.status == CheckStatus::Pass)
                .count();
            lines.push(format!(
                "Step {} \"{}\": {} — {} check{} passed, {} artifact{}, {} limitation{}.",
                index + 1,
                step.definition.title,
                match handoff.result.disposition {
                    ResultDisposition::Success => "succeeded",
                    ResultDisposition::Blocked => "blocked",
                    ResultDisposition::NeedsGuidance => "needs guidance",
                    ResultDisposition::Failed => "failed",
                },
                passed,
                if passed == 1 { "" } else { "s" },
                handoff.result.artifacts.len(),
                if handoff.result.artifacts.len() == 1 {
                    ""
                } else {
                    "s"
                },
                handoff.result.limitations.len(),
                if handoff.result.limitations.len() == 1 {
                    ""
                } else {
                    "s"
                },
            ));
        }
        let final_handoff = chain
            .steps
            .last()
            .and_then(|step| step.attempts.last())
            .and_then(|attempt| attempt.sealed.as_ref());
        if let Some(handoff) = final_handoff {
            lines.push(format!(
                "\nFinal message:\n---\n{}\n---",
                handoff.terminal_message
            ));
            if !handoff.result.limitations.is_empty() {
                lines.push(format!(
                    "Limitations: {}",
                    handoff.result.limitations.join("; ")
                ));
            }
        }
        match document
            .outcomes
            .iter()
            .find(|outcome| outcome.id == chain.request.outcome_id)
        {
            Some(outcome) if outcome.terminal() => {
                lines.push(format!(
                    "Outcome \"{}\" is {}.",
                    outcome.outcome,
                    outcome.state.label()
                ));
            }
            Some(outcome) if chain.finisher_reserved => {
                let outstanding = outcome.completion_outstanding(
                    &document.employees,
                    &document.retired_employees,
                    Uuid::nil(),
                );
                lines.push(format!(
                    "Outcome \"{}\" stays open — awaiting {}.",
                    outcome.outcome,
                    outstanding.join("; ")
                ));
            }
            _ => {}
        }
        let sender = chain
            .steps
            .last()
            .and_then(|step| step.attempts.last())
            .and_then(|attempt| attempt.employee_id);
        let trigger = sender
            .and_then(|id| self.boss.employee_including_retired(id))
            .map(|employee| {
                crate::model::ReportTrigger::new(
                    &employee,
                    crate::model::ReportTriggerKind::Finished,
                )
            });
        if let Err(error) = self.queue_agent_prompt_with_id(
            target,
            lines.join("\n"),
            sender,
            true,
            Some(chain.id),
            trigger,
            events,
        ) {
            eprintln!(
                "could not deliver chain final report {}: {error:#}",
                chain.id
            );
        }
    }

    /// The session a chain's reports land on — the chain's own supervisor
    /// when it can take prompts, else the boss session.
    fn chain_report_target(&self, chain: &EmployeeChain) -> Option<Uuid> {
        chain
            .supervisor_id
            .and_then(|id| self.boss.report_target_for(id))
            .or_else(|| self.boss.document().session_id)
    }
}
