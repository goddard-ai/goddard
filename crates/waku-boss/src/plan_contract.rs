//! Exact approved content and assignment authority; runtime adapters own delivery.
use super::*;
use waku_protocol::boss::{
    ApprovedPlanRevision, PlanAssignmentKind, PlanBinding, PlanBlockerNotice, PlanContractError,
    PlanFailureKind, PlanReadiness, PlanReadinessPhase, PlanReview,
};

type ContractResult<T> = Result<T, PlanContractError>;

fn failure(kind: PlanFailureKind, detail: impl Into<String>) -> PlanContractError {
    PlanContractError {
        kind,
        detail: detail.into(),
    }
}

fn digest(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

impl BossService {
    /// Trusted controller projection, never exposed as an employee operation.
    /// Replacement starts are explicit; late events cannot alter current readiness.
    pub fn record_plan_readiness(
        &self,
        session: Uuid,
        readiness: PlanReadiness,
        start: bool,
    ) -> anyhow::Result<()> {
        self.update(|state| {
            let employee = state
                .employees
                .iter_mut()
                .find(|e| e.session_id == session)
                .ok_or_else(|| anyhow!("required-plan employee missing"))?;
            anyhow::ensure!(
                !employee.expired()
                    && employee.assignment_kind == PlanAssignmentKind::Implementation,
                "required-plan principal is no longer active"
            );
            anyhow::ensure!(
                employee.plan_binding.is_some(),
                "required-plan binding missing"
            );
            let binding = readiness.binding.clone();
            if start {
                anyhow::ensure!(
                    employee.plan_binding.as_ref() == Some(&binding),
                    "required-plan start binding changed"
                );
            }
            if !start {
                anyhow::ensure!(
                    employee
                        .plan_readiness
                        .as_ref()
                        .is_some_and(|current| current.attempt == readiness.attempt
                            && current.runtime_generation == readiness.runtime_generation),
                    "required-plan report belongs to a retired attempt"
                );
            }
            if !start {
                anyhow::ensure!(
                    !employee
                        .plan_readiness
                        .as_ref()
                        .is_some_and(|r| r.phase == PlanReadinessPhase::Blocked)
                        || readiness.phase == PlanReadinessPhase::Blocked,
                    "blocked attempt requires fresh verified delivery"
                );
            }
            if readiness.phase == PlanReadinessPhase::Blocked {
                let error = readiness
                    .failure
                    .clone()
                    .ok_or_else(|| anyhow!("plan blocker requires a failure"))?;
                if !employee
                    .plan_blockers
                    .iter()
                    .any(|notice| notice.id == readiness.attempt)
                {
                    employee.plan_blockers.push(PlanBlockerNotice {
                        id: readiness.attempt,
                        attempt: readiness.attempt,
                        runtime_generation: readiness.runtime_generation,
                        binding,
                        error: error.clone(),
                        recorded_at: waku_protocol::model::unix_time(),
                        supervisor_notified_at: None,
                    });
                }
                employee.blocker = Some(error.detail);
            } else if start
                && employee
                    .plan_readiness
                    .as_ref()
                    .is_some_and(|r| r.phase == PlanReadinessPhase::Blocked)
            {
                employee.blocker = None;
            }
            employee.plan_readiness = Some(readiness);
            Ok(())
        })
    }

    /// Replay a controller's durable notice without replacing current readiness.
    pub fn retain_plan_blocker(
        &self,
        session: Uuid,
        notice: PlanBlockerNotice,
    ) -> anyhow::Result<()> {
        self.update(|state| {
            anyhow::ensure!(
                notice.binding.owner_id == state.identity.id,
                "foreign plan blocker owner"
            );
            let employee = state
                .employees
                .iter_mut()
                .chain(state.retired_employees.iter_mut())
                .find(|e| e.session_id == session)
                .ok_or_else(|| anyhow!("plan blocker employee missing"))?;
            if !employee.plan_blockers.iter().any(|n| n.id == notice.id) {
                employee.plan_blockers.push(notice);
            }
            Ok(())
        })
    }

    pub fn plan_blocker_notified(&self, session: Uuid, id: Uuid) -> anyhow::Result<()> {
        self.update(|state| {
            if let Some(notice) = state
                .employees
                .iter_mut()
                .chain(state.retired_employees.iter_mut())
                .find(|e| e.session_id == session)
                .and_then(|e| e.plan_blockers.iter_mut().find(|n| n.id == id))
            {
                notice
                    .supervisor_notified_at
                    .get_or_insert(waku_protocol::model::unix_time());
            }
            Ok(())
        })
    }

    /// Preview and approval both read through the same validated Boss-file path.
    pub fn preview_plan(&self, reference: &str) -> anyhow::Result<(PlanReview, String)> {
        let state = self.state.lock();
        let plan = find_plan(&state.planning, reference).ok_or_else(|| {
            failure(
                PlanFailureKind::MissingPlan,
                "plan is missing; select a plan",
            )
        })?;
        let content = if let Some(revision) = plan.approved_revisions.last() {
            let binding = PlanBinding {
                owner_id: state.identity.id,
                plan_id: plan.id,
                revision_id: Some(revision.id),
                content_digest: Some(revision.review.content_digest.clone()),
            };
            approved_revision(&state, plan, &binding)?.content.clone()
        } else {
            self.read_plan_snapshot(plan)?
        };
        Ok((
            PlanReview {
                owner_id: state.identity.id,
                plan_id: plan.id,
                content_digest: digest(&content),
            },
            content,
        ))
    }

    fn read_plan_snapshot(&self, plan: &BossPlan) -> anyhow::Result<String> {
        let read = || -> anyhow::Result<String> {
            let path = self.plan_document_path(&plan.plan_file)?;
            // A bounded read also rejects growth after the metadata check.
            use std::io::Read;
            let mut file = fs::File::open(path)?.take((MAX_FILE_BYTES + 1) as u64);
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            anyhow::ensure!(
                content.len() <= MAX_FILE_BYTES,
                "plan exceeds the Boss file limit"
            );
            Ok(content)
        };
        read().map_err(|error| failure(PlanFailureKind::CaptureFailed,
            format!("could not capture plan for approval: {error:#}; restore the document and review it again")).into())
    }

    pub(super) fn capture_plan_revision(
        &self,
        reference: &str,
        items: Option<Vec<String>>,
        now: u64,
        reviewed: &PlanReview,
    ) -> anyhow::Result<BossPlan> {
        let mut captured = None;
        // Reading and persisting approval share the state lock. The exact bytes,
        // digest and provenance live in one atomic boss.json replacement.
        self.update(|state| {
            let plan = find_plan(&state.planning, reference)
                .ok_or_else(|| failure(PlanFailureKind::MissingPlan, "plan is missing; select a plan"))?;
            if reviewed.owner_id != state.identity.id || reviewed.plan_id != plan.id {
                return Err(failure(PlanFailureKind::Conflict, "reviewed plan identity changed; reopen the plan preview").into());
            }
            if !plan.approved_revisions.is_empty() {
                return Err(failure(PlanFailureKind::Conflict, "plan is already approved; its captured revision is immutable").into());
            }
            let content = self.read_plan_snapshot(plan)?;
            if digest(&content) != reviewed.content_digest {
                return Err(failure(PlanFailureKind::ContentChanged, "plan changed since preview; review the refreshed document before approving").into());
            }
            let plan = state.planning.iter_mut().find(|plan| plan.id == reviewed.plan_id)
                .expect("plan identity checked above");
            // Legacy approval history stays intact. Reconfirmation is a new
            // human event, never a reconstruction of historical bytes.
            if plan.finalized_at.is_none() {
                apply_plan_approval(plan, items, now)?;
            } else if items.is_some() {
                return Err(failure(PlanFailureKind::Conflict,
                    "legacy reconfirmation preserves item state; update the work breakdown separately").into());
            }
            plan.approved_revisions.push(ApprovedPlanRevision {
                id: Uuid::new_v4(), review: reviewed.clone(), approved_at: now,
                actor: PlanActor::User, content,
            });
            captured = Some(plan.clone());
            Ok(())
        }).map_err(|error| {
            if error.downcast_ref::<PlanContractError>().is_some() { error } else {
                failure(PlanFailureKind::CaptureFailed,
                    format!("approval was not saved: {error:#}; retry after reviewing the plan")).into()
            }
        })?;
        captured.ok_or_else(|| anyhow!("approval capture returned no revision"))
    }

    pub fn effective_plan_binding(
        &self,
        explicit: Option<&str>,
        outcome_id: Option<Uuid>,
        item: Option<Uuid>,
        kind: PlanAssignmentKind,
    ) -> ContractResult<Option<PlanBinding>> {
        select_binding(&self.state.lock(), explicit, outcome_id, item, kind)
    }

    pub fn validate_plan_binding(&self, employee: &BossEmployee) -> ContractResult<()> {
        validate_binding(&self.state.lock(), employee)
    }

    /// Keep authority validation and a nonblocking runtime dispatch in one
    /// state lease. Revocation/rebinding cannot win a check/send race. The
    /// callback must only enqueue provider input and must not call Boss APIs.
    pub fn with_validated_plan_dispatch<T>(
        &self,
        expected: &BossEmployee,
        dispatch: impl FnOnce() -> T,
    ) -> ContractResult<T> {
        let state = self.state.lock();
        let current = state
            .employees
            .iter()
            .find(|employee| employee.session_id == expected.session_id)
            .ok_or_else(|| {
                failure(
                    PlanFailureKind::Lifecycle,
                    "plan principal is no longer active",
                )
            })?;
        if current.expired()
            || (current.lifecycle() == EmployeeLifecycle::Queued && current.ticket.is_some())
            || current.workspace_transition
            || matches!(
                current.lifecycle(),
                EmployeeLifecycle::Finishing | EmployeeLifecycle::Expired
            )
        {
            return Err(failure(
                PlanFailureKind::Lifecycle,
                "plan principal cannot execute in its current lifecycle",
            ));
        }
        let fingerprint = |employee: &BossEmployee| {
            serde_json::to_value((
                &employee.plan_binding,
                employee.assignment_kind,
                employee.item_id,
                employee.supervisor_id,
                employee.persona_id,
                &employee.permissions,
                &employee.assignment,
            ))
            .map_err(|_| {
                failure(
                    PlanFailureKind::BindingChanged,
                    "could not validate runtime assignment authority",
                )
            })
        };
        if fingerprint(current)? != fingerprint(expected)? {
            return Err(failure(
                PlanFailureKind::BindingChanged,
                "runtime assignment authority changed; review a fresh attempt",
            ));
        }
        validate_binding(&state, current)?;
        Ok(dispatch())
    }

    /// Exact-version recovery for later delivery packages; never reads live text.
    pub fn approved_plan_revision(
        &self,
        binding: &PlanBinding,
    ) -> ContractResult<ApprovedPlanRevision> {
        let state = self.state.lock();
        let plan = binding_plan(&state, binding)?;
        approved_revision(&state, plan, binding).cloned()
    }
}

fn attached_plan(state: &BossState, outcome_id: Option<Uuid>) -> ContractResult<Option<Uuid>> {
    let Some(id) = outcome_id else {
        return Ok(None);
    };
    let outcome = state
        .outcomes
        .iter()
        .find(|outcome| outcome.id == id)
        .ok_or_else(|| failure(PlanFailureKind::Lifecycle, "assignment outcome is missing"))?;
    if outcome.terminal() {
        return Err(failure(
            PlanFailureKind::Lifecycle,
            "assignment outcome is closed; reopen it before dispatch",
        ));
    }
    Ok(outcome.plan_id)
}

pub(super) fn select_binding(
    state: &BossState,
    explicit: Option<&str>,
    outcome_id: Option<Uuid>,
    item: Option<Uuid>,
    kind: PlanAssignmentKind,
) -> ContractResult<Option<PlanBinding>> {
    let explicit = explicit
        .map(|reference| {
            find_plan(&state.planning, reference)
                .map(|plan| plan.id)
                .ok_or_else(|| {
                    failure(
                        PlanFailureKind::MissingPlan,
                        "explicit plan is missing; select a plan",
                    )
                })
        })
        .transpose()?;
    let attached = attached_plan(state, outcome_id)?;
    if explicit.is_some() && attached.is_some() && explicit != attached {
        return Err(failure(
            PlanFailureKind::Conflict,
            "explicit plan conflicts with the outcome's attached plan; correct the selection",
        ));
    }
    let Some(id) = explicit.or(attached) else {
        if item.is_some() {
            return Err(failure(
                PlanFailureKind::Item,
                "item requires an explicit or attached plan",
            ));
        }
        return Ok(None);
    };
    let plan = state
        .planning
        .iter()
        .find(|plan| plan.id == id)
        .ok_or_else(|| {
            failure(
                PlanFailureKind::MissingPlan,
                "attached plan is missing; select a plan",
            )
        })?;
    validate_scope(plan, item)?;
    let revision = plan.approved_revisions.last();
    let binding = PlanBinding {
        owner_id: state.identity.id,
        plan_id: plan.id,
        revision_id: revision.map(|revision| revision.id),
        content_digest: revision.map(|revision| revision.review.content_digest.clone()),
    };
    if kind == PlanAssignmentKind::Implementation || binding.revision_id.is_some() {
        approved_revision(state, plan, &binding)?;
    }
    Ok(Some(binding))
}

fn validate_scope(plan: &BossPlan, item: Option<Uuid>) -> ContractResult<()> {
    if plan.terminal() {
        return Err(failure(
            PlanFailureKind::Lifecycle,
            "plan is closed; reopen it before assigning work",
        ));
    }
    if let Some(id) = item {
        let item = plan
            .items
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| {
                failure(
                    PlanFailureKind::Item,
                    "item does not belong to the selected plan",
                )
            })?;
        if item.state != PlanItemState::ToDo {
            return Err(failure(
                PlanFailureKind::Item,
                "item is done or dropped; reopen it before assigning work",
            ));
        }
    }
    Ok(())
}

fn binding_plan<'a>(state: &'a BossState, binding: &PlanBinding) -> ContractResult<&'a BossPlan> {
    if binding.owner_id != state.identity.id {
        return Err(failure(
            PlanFailureKind::Conflict,
            "bound plan belongs to another owner",
        ));
    }
    state
        .planning
        .iter()
        .find(|plan| plan.id == binding.plan_id)
        .ok_or_else(|| {
            failure(
                PlanFailureKind::MissingPlan,
                "bound plan is missing; restore its record",
            )
        })
}

fn approved_revision<'a>(
    state: &BossState,
    plan: &'a BossPlan,
    binding: &PlanBinding,
) -> ContractResult<&'a ApprovedPlanRevision> {
    if plan.finalized_at.is_none() {
        return Err(failure(
            PlanFailureKind::Unapproved,
            "plan is unapproved; obtain human approval or explicitly assign read-only research",
        ));
    }
    if plan.approved_revisions.is_empty() {
        return Err(failure(
            PlanFailureKind::LegacyApproval,
            "legacy approval has no verified content; human reconfirmation must capture a new revision",
        ));
    }
    let revision = plan
        .approved_revisions
        .iter()
        .find(|revision| Some(revision.id) == binding.revision_id)
        .ok_or_else(|| {
            failure(
                PlanFailureKind::Integrity,
                "bound approved revision is missing; restore the exact revision",
            )
        })?;
    if revision.review.owner_id != state.identity.id
        || revision.review.plan_id != plan.id
        || binding.content_digest.as_deref() != Some(revision.review.content_digest.as_str())
        || digest(&revision.content) != revision.review.content_digest
        || revision.actor != PlanActor::User
    {
        return Err(failure(
            PlanFailureKind::Integrity,
            "approved revision identity or digest failed; restore the exact approved copy",
        ));
    }
    Ok(revision)
}

pub(super) fn validate_binding(state: &BossState, employee: &BossEmployee) -> ContractResult<()> {
    let attached = attached_plan(
        state,
        employee
            .assignment
            .as_ref()
            .map(|assignment| assignment.outcome_id),
    )?;
    let Some(binding) = &employee.plan_binding else {
        if employee.plan_id.is_some() || employee.item_id.is_some() || attached.is_some() {
            return Err(failure(
                PlanFailureKind::BindingChanged,
                "required plan has no accepted revision binding; explicitly reassign the work",
            ));
        }
        return Ok(());
    };
    if employee.plan_id != Some(binding.plan_id) || attached.is_some_and(|id| id != binding.plan_id)
    {
        return Err(failure(
            PlanFailureKind::BindingChanged,
            "assignment links conflict with the accepted plan; explicitly reassign the work",
        ));
    }
    let plan = binding_plan(state, binding)?;
    validate_scope(plan, employee.item_id)?;
    if employee.assignment_kind == PlanAssignmentKind::Implementation
        || binding.revision_id.is_some()
    {
        approved_revision(state, plan, binding)?;
    } else if binding.content_digest.is_some() {
        return Err(failure(
            PlanFailureKind::Integrity,
            "draft research binding has an unexpected approved digest",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::boss::{EmployeeGoal, PlanItem, PlanOutcome};

    struct Fixture {
        service: BossService,
        root: PathBuf,
        boss: Uuid,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("plan-contract-{}", Uuid::new_v4()));
            let service = BossService::open(root.clone()).unwrap();
            let boss = Uuid::new_v4();
            service.set_session_id(boss).unwrap();
            Self {
                service,
                root,
                boss,
            }
        }
        fn plan(&self, name: &str, content: &str) -> BossPlan {
            let plan = BossPlan {
                id: Uuid::new_v4(),
                session_id: Uuid::new_v4(),
                plan_file: format!("plans/{name}.md"),
                idea: name.into(),
                finalized_at: None,
                approved_revisions: Vec::new(),
                items: vec![PlanItem {
                    id: Uuid::new_v4(),
                    title: "Implement".into(),
                    state: PlanItemState::ToDo,
                    history: Vec::new(),
                }],
                outcome: None,
                history: Vec::new(),
            };
            self.service.add_plan(plan.clone()).unwrap();
            self.service
                .handle(
                    None,
                    BossOperation::WriteFile {
                        path: plan.plan_file.clone(),
                        content: content.into(),
                    },
                )
                .unwrap();
            plan
        }
        fn approve(&self, plan: &BossPlan, at: u64) -> BossPlan {
            let reviewed = self.service.preview_plan(&plan.plan_file).unwrap().0;
            self.service
                .finalize_plan(&plan.plan_file, None, at, &reviewed)
                .unwrap()
        }
        fn employee(&self, plan: &BossPlan, kind: PlanAssignmentKind) -> BossEmployee {
            let mut employee = self
                .service
                .prepare_employee(
                    self.boss,
                    None,
                    "Contract fixture".into(),
                    None,
                    false,
                    EmployeeGoal::Errand,
                    None,
                )
                .unwrap();
            employee.assignment_kind = kind;
            employee.plan_id = Some(plan.id);
            employee.plan_binding = self
                .service
                .effective_plan_binding(Some(&plan.id.to_string()), None, None, kind)
                .unwrap();
            employee
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn kind(error: anyhow::Error) -> PlanFailureKind {
        error.downcast::<PlanContractError>().unwrap().kind
    }

    #[path = "plan_contract_acceptance.rs"]
    mod acceptance;

    #[test]
    fn plan_readiness_blocker_is_durable_deduplicated_and_attempt_scoped() {
        let fixture = Fixture::new();
        let plan = fixture.plan("blocker", "approved bytes");
        let plan = fixture.approve(&plan, 1);
        let employee = fixture.employee(&plan, PlanAssignmentKind::Implementation);
        let session = employee.session_id;
        let binding = employee.plan_binding.clone().unwrap();
        fixture.service.add_employee(employee).unwrap();
        let mut readiness = PlanReadiness {
            binding,
            attempt: Uuid::new_v4(),
            runtime_generation: Uuid::new_v4(),
            phase: PlanReadinessPhase::Reviewing,
            delivered: true,
            acknowledged: false,
            failure: None,
        };
        fixture
            .service
            .record_plan_readiness(session, readiness.clone(), true)
            .unwrap();
        readiness.phase = PlanReadinessPhase::Blocked;
        readiness.failure = Some(failure(
            PlanFailureKind::RuntimeAuthentication,
            "Runtime authentication failed; retry the binding",
        ));
        fixture
            .service
            .record_plan_readiness(session, readiness.clone(), false)
            .unwrap();
        fixture
            .service
            .record_plan_readiness(session, readiness.clone(), false)
            .unwrap();
        let restored = BossService::open(fixture.root.clone()).unwrap();
        let blocked = restored.employee(session).unwrap();
        assert_eq!(blocked.plan_blockers.len(), 1);
        assert_eq!(blocked.plan_readiness.as_ref().unwrap(), &readiness);
        assert!(blocked.plan_blockers[0].supervisor_notified_at.is_none());
        let mut forged_success = readiness.clone();
        forged_success.phase = PlanReadinessPhase::Ready;
        forged_success.failure = None;
        assert!(
            restored
                .record_plan_readiness(session, forged_success, false)
                .is_err()
        );
        restored
            .plan_blocker_notified(session, readiness.attempt)
            .unwrap();
        let historical = restored.employee(session).unwrap().plan_blockers[0].clone();
        let mut replacement = readiness.clone();
        replacement.attempt = Uuid::new_v4();
        replacement.runtime_generation = Uuid::new_v4();
        replacement.phase = PlanReadinessPhase::Preparing;
        replacement.failure = None;
        restored
            .record_plan_readiness(session, replacement.clone(), true)
            .unwrap();
        assert!(
            restored
                .record_plan_readiness(session, readiness, false)
                .is_err()
        );
        restored.retain_plan_blocker(session, historical).unwrap();
        let current = restored.employee(session).unwrap();
        assert_eq!(current.plan_readiness.as_ref().unwrap(), &replacement);
        assert!(current.blocker.is_none());
        assert_eq!(current.plan_blockers.len(), 1);
        assert!(current.plan_blockers[0].supervisor_notified_at.is_some());
        let mut blocked_again = current.plan_readiness.unwrap_or(replacement);
        blocked_again.phase = PlanReadinessPhase::Blocked;
        blocked_again.failure = Some(failure(
            PlanFailureKind::RuntimeUnavailable,
            "Runtime unavailable",
        ));
        restored
            .record_plan_readiness(session, blocked_again, false)
            .unwrap();
        restored.clear_employee_blocker(session).unwrap();
        let finished = restored
            .begin_finishing(session, false, true, ExpiryCause::Finished)
            .unwrap()
            .unwrap();
        assert_eq!(finished.expiry.unwrap().cause, ExpiryCause::Failed);
        assert!(finished.blocker.is_some());
    }

    #[test]
    fn approval_captures_exact_bytes_and_survives_live_file_change_and_restart() {
        let fixture = Fixture::new();
        let text = "# Exact\r\nπ and trailing spaces  \n";
        let plan = fixture.plan("exact", text);
        let approved = fixture.approve(&plan, 42);
        let revision = &approved.approved_revisions[0];
        assert_eq!(revision.content, text);
        assert_eq!(
            revision.review.content_digest,
            "e557c1bf24b0b07ba5447cf7556e5182c73e4f50970431959596bf40f0288785"
        );
        assert_eq!(revision.approved_at, 42);
        assert_eq!(revision.actor, PlanActor::User);
        assert_eq!(
            revision.review.owner_id,
            fixture.service.document().identity.id
        );
        assert_eq!(revision.review.plan_id, plan.id);
        let binding = fixture
            .employee(&plan, PlanAssignmentKind::Implementation)
            .plan_binding
            .unwrap();
        fs::write(
            fixture.service.plan_document_path(&plan.plan_file).unwrap(),
            "unreviewed external replacement",
        )
        .unwrap();
        assert_eq!(
            fixture.service.preview_plan(&plan.plan_file).unwrap().1,
            text
        );
        let restored = BossService::open(fixture.root.clone()).unwrap();
        assert_eq!(
            restored.approved_plan_revision(&binding).unwrap().content,
            text
        );
        assert_eq!(
            restored.approved_plan_revision(&binding).unwrap().id,
            revision.id
        );
        // Copy integrity is checked independently of the mutable document path.
        restored
            .update(|state| {
                state.planning[0].approved_revisions[0].content.push('!');
                Ok(())
            })
            .unwrap();
        assert_eq!(
            restored.approved_plan_revision(&binding).unwrap_err().kind,
            PlanFailureKind::Integrity
        );
    }

    #[test]
    fn runtime_dispatch_rejects_changed_grants_and_withdrawn_approval() {
        let fixture = Fixture::new();
        let plan = fixture.plan("runtime", "approved bytes");
        let plan = fixture.approve(&plan, 1);
        let employee = fixture.employee(&plan, PlanAssignmentKind::Implementation);
        fixture.service.add_employee(employee.clone()).unwrap();
        let mut starts = 0;
        fixture
            .service
            .with_validated_plan_dispatch(&employee, || starts += 1)
            .unwrap();
        fixture
            .service
            .update(|state| {
                state.employees[0].permissions.computer_use = !employee.permissions.computer_use;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            fixture
                .service
                .with_validated_plan_dispatch(&employee, || starts += 1)
                .unwrap_err()
                .kind,
            PlanFailureKind::BindingChanged
        );
        fixture
            .service
            .update(|state| {
                state.employees[0].permissions = employee.permissions.clone();
                state.planning[0].finalized_at = None;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            fixture
                .service
                .with_validated_plan_dispatch(&employee, || starts += 1)
                .unwrap_err()
                .kind,
            PlanFailureKind::Unapproved
        );
        assert_eq!(starts, 1);
    }

    #[test]
    fn changed_preview_and_failed_capture_never_approve() {
        let fixture = Fixture::new();
        let plan = fixture.plan("draft", "reviewed");
        let reviewed = fixture.service.preview_plan(&plan.plan_file).unwrap().0;
        let file = fixture.service.plan_document_path(&plan.plan_file).unwrap();
        fs::write(&file, "unreviewed").unwrap();
        assert_eq!(
            kind(
                fixture
                    .service
                    .finalize_plan(&plan.plan_file, None, 1, &reviewed)
                    .unwrap_err()
            ),
            PlanFailureKind::ContentChanged
        );
        assert!(
            fixture
                .service
                .plan_for_file(&plan.plan_file)
                .unwrap()
                .finalized_at
                .is_none()
        );
        fs::remove_file(&file).unwrap();
        assert_eq!(
            kind(
                fixture
                    .service
                    .finalize_plan(&plan.plan_file, None, 1, &reviewed)
                    .unwrap_err()
            ),
            PlanFailureKind::CaptureFailed
        );
        assert!(
            fixture
                .service
                .plan_for_file(&plan.plan_file)
                .unwrap()
                .approved_revisions
                .is_empty()
        );
        fs::write(&file, "x".repeat(MAX_FILE_BYTES + 1)).unwrap();
        assert_eq!(
            kind(fixture.service.preview_plan(&plan.plan_file).unwrap_err()),
            PlanFailureKind::CaptureFailed
        );
        // Owner identity is part of the reviewed bundle, even with equal bytes.
        fs::write(&file, "reviewed").unwrap();
        let mut wrong_owner = reviewed.clone();
        wrong_owner.owner_id = Uuid::new_v4();
        assert_eq!(
            kind(
                fixture
                    .service
                    .finalize_plan(&plan.plan_file, None, 1, &wrong_owner)
                    .unwrap_err()
            ),
            PlanFailureKind::Conflict
        );
        assert!(
            fixture
                .service
                .plan_for_file(&plan.plan_file)
                .unwrap()
                .history
                .is_empty()
        );
    }

    #[test]
    fn failed_atomic_save_leaves_no_approval_in_memory_or_on_disk() {
        let fixture = Fixture::new();
        let plan = fixture.plan("save", "reviewed");
        let reviewed = fixture.service.preview_plan(&plan.plan_file).unwrap().0;
        let state_path = fixture.root.join("boss.json");
        let before = fs::read(&state_path).unwrap();
        fs::remove_file(&state_path).unwrap();
        fs::create_dir(&state_path).unwrap();
        assert_eq!(
            kind(
                fixture
                    .service
                    .finalize_plan(&plan.plan_file, None, 1, &reviewed)
                    .unwrap_err()
            ),
            PlanFailureKind::CaptureFailed
        );
        let unchanged = fixture.service.plan_for_file(&plan.plan_file).unwrap();
        assert!(unchanged.finalized_at.is_none());
        assert!(unchanged.approved_revisions.is_empty());
        assert!(unchanged.history.is_empty());
        fs::remove_dir(&state_path).unwrap();
        fs::write(&state_path, before).unwrap();
        let restored = BossService::open(fixture.root.clone()).unwrap();
        assert!(
            restored
                .plan_for_file(&plan.plan_file)
                .unwrap()
                .finalized_at
                .is_none()
        );
        fixture.approve(&plan, 2);
    }

    #[test]
    fn legacy_approval_requires_new_human_revision_without_reopening_outcome() {
        let fixture = Fixture::new();
        let plan = fixture.plan("legacy", "current bytes, historical bytes unknown");
        fixture
            .service
            .set_plan_finalized_at(plan.session_id, 10)
            .unwrap();
        fixture
            .service
            .set_plan_outcome(None, &plan.plan_file, PlanOutcome::Completed)
            .unwrap();
        let before = fixture.service.plan_for_file(&plan.plan_file).unwrap();
        fixture
            .service
            .set_plan_outcome(None, &plan.plan_file, PlanOutcome::Approved)
            .unwrap();
        assert_eq!(
            fixture
                .service
                .effective_plan_binding(
                    Some(&plan.plan_file),
                    None,
                    None,
                    PlanAssignmentKind::Implementation
                )
                .unwrap_err()
                .kind,
            PlanFailureKind::LegacyApproval
        );
        fixture
            .service
            .set_plan_outcome(None, &plan.plan_file, PlanOutcome::Completed)
            .unwrap();
        let history = fixture
            .service
            .plan_for_file(&plan.plan_file)
            .unwrap()
            .history;
        let reconfirmed = fixture.approve(&plan, 99);
        assert_eq!(reconfirmed.finalized_at, before.finalized_at);
        assert_eq!(reconfirmed.outcome, Some(PlanOutcome::Completed));
        assert_eq!(reconfirmed.history, history);
        assert_eq!(reconfirmed.approved_revisions.len(), 1);
        assert_eq!(reconfirmed.approved_revisions[0].approved_at, 99);
        assert_eq!(
            reconfirmed.approved_revisions[0].content,
            "current bytes, historical bytes unknown"
        );
    }

    #[test]
    fn effective_binding_inherits_plan_checks_items_and_requires_explicit_research() {
        let fixture = Fixture::new();
        let draft = fixture.plan("draft", "draft");
        assert_eq!(
            fixture
                .service
                .effective_plan_binding(
                    Some("draft.md"),
                    None,
                    None,
                    PlanAssignmentKind::Implementation
                )
                .unwrap_err()
                .kind,
            PlanFailureKind::Unapproved
        );
        let research = fixture.employee(&draft, PlanAssignmentKind::Research);
        assert!(
            research
                .plan_binding
                .as_ref()
                .unwrap()
                .revision_id
                .is_none()
        );
        fixture.service.validate_plan_binding(&research).unwrap();
        fixture.service.add_employee(research.clone()).unwrap();
        assert!(
            fixture
                .service
                .prompt_with_context(research.session_id, "Investigate".into())
                .contains("Research only")
        );
        let plan = fixture.approve(&draft, 1);
        let other = fixture.plan("other", "other");
        fixture.approve(&other, 2);
        let outcome = fixture
            .service
            .create_outcome(None, "Bound outcome", "Verify the work")
            .unwrap();
        fixture
            .service
            .attach_plan(None, outcome.id, &plan.plan_file)
            .unwrap();
        let inherited = fixture
            .service
            .effective_plan_binding(
                None,
                Some(outcome.id),
                Some(plan.items[0].id),
                PlanAssignmentKind::Implementation,
            )
            .unwrap()
            .unwrap();
        assert_eq!(inherited.plan_id, plan.id);
        assert_eq!(
            fixture
                .service
                .effective_plan_binding(
                    Some(&other.plan_file),
                    Some(outcome.id),
                    None,
                    PlanAssignmentKind::Implementation
                )
                .unwrap_err()
                .kind,
            PlanFailureKind::Conflict
        );
        assert_eq!(
            fixture
                .service
                .effective_plan_binding(
                    None,
                    Some(outcome.id),
                    Some(other.items[0].id),
                    PlanAssignmentKind::Implementation
                )
                .unwrap_err()
                .kind,
            PlanFailureKind::Item
        );
        assert!(
            fixture
                .service
                .effective_plan_binding(None, None, None, PlanAssignmentKind::Implementation)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            fixture
                .service
                .effective_plan_binding(
                    None,
                    None,
                    Some(plan.items[0].id),
                    PlanAssignmentKind::Research
                )
                .unwrap_err()
                .kind,
            PlanFailureKind::Item
        );
        fixture
            .service
            .set_plan_item_state(None, &plan.plan_file, plan.items[0].id, PlanItemState::Done)
            .unwrap();
        assert_eq!(
            fixture
                .service
                .effective_plan_binding(
                    None,
                    Some(outcome.id),
                    Some(plan.items[0].id),
                    PlanAssignmentKind::Implementation
                )
                .unwrap_err()
                .kind,
            PlanFailureKind::Item
        );
    }

    #[test]
    fn accepted_revision_is_pinned_and_legacy_assignments_cannot_auto_bind() {
        let fixture = Fixture::new();
        let draft = fixture.plan("pinned", "first");
        let plan = fixture.approve(&draft, 1);
        let mut employee = fixture.employee(&plan, PlanAssignmentKind::Implementation);
        fixture.service.add_employee(employee.clone()).unwrap();
        let binding = employee.plan_binding.clone().unwrap();
        // Simulate a separately approved successor, never selection of latest.
        fixture
            .service
            .update(|state| {
                let mut successor = state.planning[0].approved_revisions[0].clone();
                successor.id = Uuid::new_v4();
                successor.content = "second".into();
                successor.review.content_digest = digest(&successor.content);
                successor.approved_at = 2;
                state.planning[0].approved_revisions.push(successor);
                Ok(())
            })
            .unwrap();
        fixture.service.validate_plan_binding(&employee).unwrap();
        fixture
            .service
            .set_employee_plan(employee.session_id, Some(Some("pinned.md".into())), None)
            .unwrap();
        assert_eq!(
            fixture
                .service
                .employee(employee.session_id)
                .unwrap()
                .plan_binding,
            Some(binding.clone())
        );
        assert_eq!(
            fixture
                .service
                .approved_plan_revision(&binding)
                .unwrap()
                .content,
            "first"
        );
        let other = fixture.plan("replacement", "replacement");
        fixture.approve(&other, 3);
        assert_eq!(
            kind(
                fixture
                    .service
                    .set_employee_plan(employee.session_id, Some(Some(other.plan_file)), None)
                    .unwrap_err()
            ),
            PlanFailureKind::BindingChanged
        );
        employee.plan_binding = None;
        fixture
            .service
            .update(|state| {
                state.employees[0].plan_binding = None;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            kind(
                fixture
                    .service
                    .set_employee_plan(employee.session_id, None, None)
                    .unwrap_err()
            ),
            PlanFailureKind::BindingChanged
        );
        assert_eq!(
            fixture
                .service
                .validate_plan_binding(&employee)
                .unwrap_err()
                .kind,
            PlanFailureKind::BindingChanged
        );
        employee.plan_binding = Some(binding);
        fixture
            .service
            .update(|state| {
                state.planning[0].finalized_at = None;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            fixture
                .service
                .validate_plan_binding(&employee)
                .unwrap_err()
                .kind,
            PlanFailureKind::Unapproved
        );
        // No runtime revalidation may repair approval by observing current bytes.
        assert_eq!(
            fixture
                .service
                .plan_for_file(&plan.plan_file)
                .unwrap()
                .approved_revisions
                .len(),
            2
        );
    }
}
