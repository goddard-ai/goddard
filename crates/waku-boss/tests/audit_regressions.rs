use std::{fs, path::PathBuf, sync::Barrier};
use uuid::Uuid;
use waku_boss::{
    BossService,
    chains::{ChainObservation, HandoffVerification},
};
use waku_protocol::{boss::*, chains::*};

struct Fixture {
    root: PathBuf,
    service: BossService,
    boss: Uuid,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("boss-audit-regression-{}", Uuid::new_v4()));
        let service = BossService::open(root.clone()).unwrap();
        let boss = Uuid::new_v4();
        service.set_session_id(boss).unwrap();
        Self {
            root,
            service,
            boss,
        }
    }
    fn employee(&self, title: &str) -> BossEmployee {
        self.service
            .prepare_employee(
                self.boss,
                None,
                title.into(),
                None,
                false,
                EmployeeGoal::Errand,
                None,
            )
            .unwrap()
    }
    fn chain(&self) -> EmployeeChain {
        let outcome = self
            .service
            .create_outcome(None, "audit", "evidence")
            .unwrap();
        let definition = serde_json::from_value(serde_json::json!({
            "outcomeId":outcome.id,"finishesOutcome":false,"steps":[{
                "purpose":"helping","title":"audit step","prompt":"audit prompt",
                "projectId":Uuid::new_v4(),"personaId":null,"provider":"codex","model":"test-model",
                "reasoningEffort":null,"permissions":PersonaPermissions::default(),"resources":{},
                "workspace":"local","baseRevision":null,"boundary":"artifactsReady",
                "requiredChecks":[],"requiredArtifacts":[],"requiredApprovals":[],"maxRetries":0,
                "planId":null,"itemId":null,"successorBrief":""
            }]
        }))
        .unwrap();
        self.service
            .chain_operation(
                None,
                ChainOperation::Create {
                    request_id: Uuid::new_v4(),
                    definition,
                },
            )
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn ticket(prompt: &str) -> SummonTicket {
    serde_json::from_value(serde_json::json!({
        "sequence":0,"generation":1,"provider":"codex","model":"test-model","prompt":prompt,"project":"/tmp"
    })).unwrap()
}

#[test]
fn employee_view_scopes_retired_assignment_prompts() {
    let f = Fixture::new();
    let reader = f.employee("reader");
    let reader_id = reader.session_id;
    f.service
        .enqueue_ticket(reader, ticket("reader assignment"))
        .unwrap();
    let foreign = f.employee("private");
    let foreign_id = foreign.session_id;
    f.service
        .enqueue_ticket(foreign, ticket("unrelated private prompt"))
        .unwrap();
    let mut child = f.employee("child");
    child.supervisor_id = reader_id;
    let child_id = child.session_id;
    f.service
        .enqueue_ticket(child, ticket("supervised prompt"))
        .unwrap();
    for id in [foreign_id, child_id] {
        f.service.set_employee_expired_at(id, 1).unwrap();
    }
    f.service.retire_expired(10_000).unwrap();
    let BossResult::State { state } = f
        .service
        .handle(Some(reader_id), BossOperation::View)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(state.retired_employees.len(), 1);
    assert_eq!(state.retired_employees[0].session_id, child_id);
    assert!(
        !serde_json::to_string(&state)
            .unwrap()
            .contains("unrelated private prompt")
    );
}

#[test]
fn concurrent_chain_admission_publishes_one_bound_employee() {
    let f = Fixture::new();
    let chain = f.chain();
    let attempt = chain.steps[0].attempts[0].clone();
    let barrier = Barrier::new(2);
    let ids = std::thread::scope(|scope| {
        let handles = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    let mut employee = f.employee("audit step");
                    employee.request_id = Some(attempt.dispatch_id);
                    employee.request_fingerprint = Some("same accepted step".into());
                    barrier.wait();
                    f.service
                        .enqueue_chain_employee(
                            chain.id,
                            attempt.id,
                            attempt.definition_version,
                            employee,
                            ticket("audit prompt"),
                        )
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(ids[0], ids[1]);
    let state = f.service.document();
    assert_eq!(state.employees.len(), 1);
    assert_eq!(
        state.chains[0].steps[0].attempts[0].employee_id,
        Some(ids[0])
    );
    let restarted = BossService::open(f.root.clone()).unwrap();
    assert_eq!(
        restarted.document().chains[0].steps[0].attempts[0].employee_id,
        Some(ids[0])
    );
}

#[test]
fn repaired_gate_resume_checks_authority_and_revision_before_mutation() {
    let f = Fixture::new();
    let chain = f.chain();
    let attempt = chain.steps[0].attempts[0].clone();
    let observe = |observation| {
        f.service
            .observe_chain(
                chain.id,
                attempt.id,
                attempt.definition_version,
                Uuid::new_v4(),
                observation,
            )
            .unwrap()
    };
    observe(ChainObservation::Admitted {
        employee_id: Uuid::new_v4(),
    });
    f.service
        .chain_operation(
            None,
            ChainOperation::RecordResult {
                chain_id: chain.id,
                attempt_id: attempt.id,
                definition_version: attempt.definition_version,
                result: ChainResult {
                    disposition: ResultDisposition::Success,
                    checks: vec![],
                    artifacts: vec![],
                    code: None,
                    limitations: vec![],
                },
            },
        )
        .unwrap();
    observe(ChainObservation::Terminal {
        terminal: ChainTerminal {
            message: "done".into(),
            disposition: ResultDisposition::Success,
        },
    });
    observe(ChainObservation::Released {
        execution: true,
        workspace: true,
    });
    f.service
        .seal_chain_handoff(
            chain.id,
            attempt.id,
            attempt.definition_version,
            &Default::default(),
        )
        .unwrap();
    let before = f.service.document();
    let revision = before.chains[0].revision;
    for (caller, expected_revision) in [(Some(Uuid::new_v4()), revision), (None, revision - 1)] {
        let action = ChainOperation::Control {
            chain_id: chain.id,
            expected_revision,
            control: ChainControl::Resume,
            source_revision: None,
        };
        assert!(
            f.service
                .chain_operation_with_preflight(
                    caller,
                    action,
                    None,
                    Some((
                        attempt.id,
                        attempt.definition_version,
                        HandoffVerification {
                            successor_access: true,
                            ..Default::default()
                        }
                    ))
                )
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(f.service.document()).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
    }
    let action = ChainOperation::Control {
        chain_id: chain.id,
        expected_revision: revision,
        control: ChainControl::Resume,
        source_revision: None,
    };
    let resumed = f
        .service
        .chain_operation_with_preflight(
            None,
            action,
            None,
            Some((
                attempt.id,
                attempt.definition_version,
                HandoffVerification {
                    successor_access: true,
                    ..Default::default()
                },
            )),
        )
        .unwrap();
    assert_eq!(resumed.status, ChainStatus::Completed);
}

#[test]
fn failed_deliverable_updates_preserve_the_committed_bytes() {
    for reference in [false, true] {
        let f = Fixture::new();
        let source = f.root.join("report.txt");
        fs::write(&source, "original").unwrap();
        let publish = |reference| BossOperation::PublishDeliverable {
            path: source.display().to_string(),
            name: None,
            reference,
        };
        f.service.handle(None, publish(false)).unwrap();
        let committed = f.service.document().deliverables[0].clone();
        fs::write(&source, "replacement").unwrap();
        fs::remove_file(f.root.join("boss.json")).unwrap();
        fs::create_dir(f.root.join("boss.json")).unwrap();
        assert!(
            f.service
                .handle(
                    None,
                    BossOperation::PublishDeliverable {
                        path: source.display().to_string(),
                        name: Some("replacement".into()),
                        reference,
                    },
                )
                .is_err()
        );
        assert_eq!(fs::read_to_string(&committed.path).unwrap(), "original");
        assert_eq!(f.service.document().deliverables[0].path, committed.path);
    }
}

#[test]
fn displayed_queue_positions_follow_priority_and_force_start() {
    let f = Fixture::new();
    let first = f.employee("first");
    let first_id = first.session_id;
    f.service.enqueue_ticket(first, ticket("first")).unwrap();
    let second = f.employee("second");
    let second_id = second.session_id;
    let mut high = ticket("second");
    high.priority = Some(100);
    f.service.enqueue_ticket(second, high).unwrap();
    assert_eq!(f.service.queued_heads()[0].session_id, second_id);
    assert_eq!(f.service.queue_position(second_id), Some(1));
    f.service.force_start_employee(first_id).unwrap();
    assert_eq!(f.service.queued_heads()[0].session_id, first_id);
    assert_eq!(f.service.queue_position(first_id), Some(1));
    assert_eq!(f.service.queue_position(second_id), Some(2));
}

#[test]
fn completion_and_report_commit_together_and_survive_readmission() {
    let f = Fixture::new();
    let employee = f.employee("reporter");
    let id = employee.session_id;
    f.service.enqueue_ticket(employee, ticket("work")).unwrap();
    f.service.mark_dispatching(id, 1, None).unwrap();
    f.service.mark_working(id, 1).unwrap();
    f.service
        .begin_finishing(id, false, false, ExpiryCause::Finished)
        .unwrap();
    let expiry = EmployeeExpiry::settle(ExpiryCause::Finished);
    fs::remove_file(f.root.join("boss.json")).unwrap();
    fs::create_dir(f.root.join("boss.json")).unwrap();
    assert!(
        f.service
            .complete_expiry_with_report(
                id,
                expiry.clone(),
                "index".into(),
                false,
                "done".into(),
                EmployeeSettle::TurnFinished
            )
            .is_err()
    );
    assert_eq!(
        f.service.employee(id).unwrap().lifecycle(),
        EmployeeLifecycle::Finishing
    );
    assert!(f.service.pending_completion_reports().is_empty());
    fs::remove_dir(f.root.join("boss.json")).unwrap();
    let complete = f
        .service
        .complete_expiry_with_report(
            id,
            expiry,
            "index".into(),
            false,
            "done".into(),
            EmployeeSettle::TurnFinished,
        )
        .unwrap();
    let report_id = complete.report_id.unwrap();
    f.service
        .requeue_employee(id, ticket("next"), |_| {})
        .unwrap();
    let reopened = BossService::open(f.root.clone()).unwrap();
    let reports = reopened.pending_completion_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].id, report_id);
    assert_eq!(reports[0].employee.ticket.as_ref().unwrap().generation, 1);
    assert_eq!(reports[0].body, "index");
    reopened.completion_report_delivered(report_id).unwrap();
    assert!(reopened.pending_completion_reports().is_empty());
}

#[test]
fn acknowledged_completion_cannot_be_recreated_by_a_late_finish() {
    let f = Fixture::new();
    let employee = f.employee("reporter");
    let id = employee.session_id;
    f.service.enqueue_ticket(employee, ticket("work")).unwrap();
    f.service.mark_dispatching(id, 1, None).unwrap();
    f.service.mark_working(id, 1).unwrap();
    f.service
        .begin_finishing(id, false, false, ExpiryCause::Finished)
        .unwrap();
    let finish = || {
        f.service
            .complete_expiry_with_report(
                id,
                EmployeeExpiry::settle(ExpiryCause::Finished),
                "index".into(),
                false,
                "done".into(),
                EmployeeSettle::TurnFinished,
            )
            .unwrap()
    };
    let first = finish().report_id.unwrap();
    f.service.completion_report_delivered(first).unwrap();
    assert!(finish().report_id.is_none());
    assert!(f.service.pending_completion_reports().is_empty());
}

#[test]
fn deliverable_recovery_preserves_stable_paths_at_publication_boundaries() {
    for boundary in ["uncommitted", "publishing", "first"] {
        let f = Fixture::new();
        let source = f.root.join("report.txt");
        fs::write(&source, "original").unwrap();
        f.service
            .handle(
                None,
                BossOperation::PublishDeliverable {
                    path: source.display().to_string(),
                    name: None,
                    reference: false,
                },
            )
            .unwrap();
        let record = f.service.document().deliverables[0].clone();
        let snapshots = f.root.join("deliverables");
        let target = snapshots.join(record.id.to_string());
        match boundary {
            "uncommitted" => {
                let stage = snapshots.join(format!(".stage-{}", record.id));
                fs::create_dir(&stage).unwrap();
                fs::write(stage.join("report.txt"), "uncommitted replacement").unwrap();
            }
            "publishing" => {
                let publish = snapshots.join(format!(".publish-{}", record.id));
                fs::create_dir(&publish).unwrap();
                fs::write(publish.join("report.txt"), "replacement").unwrap();
                fs::rename(&target, snapshots.join(format!(".backup-{}", record.id))).unwrap();
            }
            "first" => {
                fs::rename(&target, snapshots.join(format!(".stage-{}", record.id))).unwrap();
            }
            _ => unreachable!(),
        }
        let reopened = BossService::open(f.root.clone()).unwrap();
        assert_eq!(reopened.document().deliverables[0].path, record.path);
        assert_eq!(
            fs::read_to_string(&record.path).unwrap(),
            if boundary == "publishing" {
                "replacement"
            } else {
                "original"
            }
        );
    }
}
