use super::*;

#[test]
fn plan_acceptance_same_path_foreign_owner_cannot_reuse_revision() {
    let owner = Fixture::new();
    let foreign = Fixture::new();
    let plan = owner.plan("same", "OWNER_BYTES");
    let plan = owner.approve(&plan, 1);
    let other = foreign.plan("same", "FOREIGN_BYTES");
    foreign.approve(&other, 1);
    let binding = owner
        .employee(&plan, PlanAssignmentKind::Implementation)
        .plan_binding
        .unwrap();
    assert_eq!(
        foreign
            .service
            .approved_plan_revision(&binding)
            .unwrap_err()
            .kind,
        PlanFailureKind::Conflict
    );
    assert_eq!(
        owner
            .service
            .approved_plan_revision(&binding)
            .unwrap()
            .content,
        "OWNER_BYTES"
    );
    let mut wrong_revision = binding.clone();
    wrong_revision.revision_id = Some(Uuid::new_v4());
    assert_eq!(
        owner
            .service
            .approved_plan_revision(&wrong_revision)
            .unwrap_err()
            .kind,
        PlanFailureKind::Integrity
    );
    let mut wrong_digest = binding;
    wrong_digest.content_digest = Some("wrong".into());
    assert_eq!(
        owner
            .service
            .approved_plan_revision(&wrong_digest)
            .unwrap_err()
            .kind,
        PlanFailureKind::Integrity
    );
}

#[test]
fn plan_acceptance_selected_delivery_does_not_expand_private_links() {
    let fixture = Fixture::new();
    fixture
        .service
        .handle(
            None,
            BossOperation::WriteFile {
                path: "docs/private.md".into(),
                content: "PRIVATE_REFERENCE_SENTINEL".into(),
            },
        )
        .unwrap();
    let plan = fixture.plan(
        "selected",
        "# Shared plan\n[Restricted reference](../docs/private.md)\n",
    );
    let plan = fixture.approve(&plan, 1);
    let employee = fixture.employee(&plan, PlanAssignmentKind::Implementation);
    fixture.service.add_employee(employee.clone()).unwrap();
    let delivered = fixture
        .service
        .approved_plan_revision(employee.plan_binding.as_ref().unwrap())
        .unwrap();
    assert!(delivered.content.contains("Restricted reference"));
    assert!(
        !serde_json::to_string(&delivered)
            .unwrap()
            .contains("PRIVATE_REFERENCE_SENTINEL")
    );
    assert!(
        fixture
            .service
            .handle(
                Some(employee.session_id),
                BossOperation::ReadFile {
                    path: "docs/private.md".into()
                }
            )
            .is_err()
    );
    assert!(
        fixture
            .service
            .handle(
                Some(Uuid::new_v4()),
                BossOperation::ReadFile {
                    path: plan.plan_file
                }
            )
            .is_err()
    );
    // Link expansion/essential attachment validation is not implemented here;
    // this probe establishes only that delivery cannot publish private bytes.
}

#[test]
fn plan_acceptance_pending_notice_reopens_and_ack_replay_is_idempotent() {
    let fixture = Fixture::new();
    let plan = fixture.plan("pending", "approved");
    let plan = fixture.approve(&plan, 1);
    let employee = fixture.employee(&plan, PlanAssignmentKind::Implementation);
    let session = employee.session_id;
    let binding = employee.plan_binding.clone().unwrap();
    fixture.service.add_employee(employee).unwrap();
    let readiness = PlanReadiness {
        binding,
        attempt: Uuid::new_v4(),
        runtime_generation: Uuid::new_v4(),
        phase: PlanReadinessPhase::Blocked,
        delivered: false,
        acknowledged: false,
        failure: Some(failure(
            PlanFailureKind::RuntimeUnavailable,
            "controlled report transport outage",
        )),
    };
    fixture
        .service
        .record_plan_readiness(session, readiness.clone(), true)
        .unwrap();
    let reopened = BossService::open(fixture.root.clone()).unwrap();
    let notice = reopened.employee(session).unwrap().plan_blockers[0].clone();
    assert!(notice.supervisor_notified_at.is_none());
    for _ in 0..3 {
        reopened
            .record_plan_readiness(session, readiness.clone(), false)
            .unwrap();
        reopened.plan_blocker_notified(session, notice.id).unwrap();
    }
    let restarted = BossService::open(fixture.root.clone()).unwrap();
    let notices = restarted.employee(session).unwrap().plan_blockers;
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].id, notice.id);
    assert!(notices[0].supervisor_notified_at.is_some());
    assert_eq!(
        restarted
            .employee(session)
            .unwrap()
            .plan_readiness
            .unwrap()
            .phase,
        PlanReadinessPhase::Blocked
    );
}

#[test]
fn plan_acceptance_prerequisite_wait_does_not_change_approved_binding() {
    use waku_protocol::boss::ExpiryCause;
    let fixture = Fixture::new();
    let plan = fixture.plan("prerequisite", "approved bytes");
    let plan = fixture.approve(&plan, 1);
    let outcome = fixture
        .service
        .create_outcome(None, "Synthetic task", "completed fixture")
        .unwrap();
    fixture
        .service
        .attach_plan(None, outcome.id, &plan.plan_file)
        .unwrap();
    let mut worker = fixture.employee(&plan, PlanAssignmentKind::Implementation);
    worker.assignment = fixture
        .service
        .assignment_admission(Some(outcome.id), None, false, None, Vec::new(), None)
        .unwrap();
    fixture.service.add_employee(worker.clone()).unwrap();
    let mut dependent = fixture.employee(&plan, PlanAssignmentKind::Implementation);
    dependent.assignment = fixture
        .service
        .assignment_admission(
            Some(outcome.id),
            None,
            false,
            None,
            vec![worker.session_id],
            None,
        )
        .unwrap();
    fixture.service.add_employee(dependent.clone()).unwrap();
    let binding = dependent.plan_binding.clone();
    assert!(!fixture.service.assignment_ready(&dependent));
    fixture.service.validate_plan_binding(&dependent).unwrap();
    assert_eq!(
        fixture
            .service
            .employee(dependent.session_id)
            .unwrap()
            .plan_binding,
        binding
    );
    fixture
        .service
        .begin_finishing(worker.session_id, false, false, ExpiryCause::Finished)
        .unwrap();
    fixture
        .service
        .complete_expiry(worker.session_id, None)
        .unwrap();
    assert!(fixture.service.assignment_ready(&dependent));
    fixture.service.validate_plan_binding(&dependent).unwrap();
    assert_eq!(
        fixture
            .service
            .employee(dependent.session_id)
            .unwrap()
            .plan_binding,
        binding
    );
}
