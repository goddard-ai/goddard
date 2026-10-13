use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use waku_protocol::boss::{AdmissionBlocker, EmployeeControl, EmployeeLifecycle};
use waku_protocol::resources::ResourceSet;

fn fixture(root: &Path) -> (WakuBackend, Uuid, Arc<CaptureDriver>, Arc<AtomicUsize>) {
    let (mut backend, boss) = summon_test_backend(root);
    backend.set_daemon_address("http://127.0.0.1:1".into());
    backend.runtime_test_cli = Some("/usr/bin/true".into());
    let mut settings = backend.settings.get();
    settings
        .provider_binary_overrides
        .insert(ProviderKind::Codex, "/usr/bin/true".into());
    backend.settings.replace(settings).unwrap();
    let capture = Arc::new(CaptureDriver::default());
    let launches = Arc::new(AtomicUsize::new(0));
    let driver = capture.clone();
    let count = launches.clone();
    backend.runtime_test_start = Some(Arc::new(move |_, _, _| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(DriverHandle::from_control(driver.clone()))
    }));
    (backend, boss, capture, launches)
}

fn queued(
    backend: &WakuBackend,
    boss: Uuid,
    root: &Path,
    title: &str,
    resources: ResourceSet,
) -> Uuid {
    let mut operation = summon_op(backend, root, title, ProviderKind::Codex, Some("gpt-5.5"));
    if let BossOperation::Summon {
        workspace,
        resources: declared,
        ..
    } = &mut operation
    {
        *workspace = Some(AgentWorkspace::Local);
        *declared = Some(resources);
    }
    let BossResult::Summoned {
        session_id, state, ..
    } = backend
        .handle_boss_operation(Some(boss), operation, &EventSink::detached())
        .unwrap()
    else {
        panic!("expected summon")
    };
    assert_eq!(state, EmployeeLifecycle::Queued);
    session_id
}

fn submit(
    backend: &WakuBackend,
    employee: Uuid,
    submission: Uuid,
    prompt: &str,
) -> anyhow::Result<ResponsePayload> {
    backend.handle(
        Request {
            request_id: Uuid::new_v4(),
            session_id: employee,
            runtime_id: Uuid::nil(),
            command: Command::EmployeeComposerPrompt {
                viewed_employee_id: employee,
                submission_id: submission,
                prompt: prompt.into(),
                delivery: AgentPromptDelivery::Queue,
            },
        },
        EventSink::detached(),
        None,
    )
}

#[test]
fn employee_composer_bypasses_soft_hard_and_zero_model_caps_once() {
    for (soft, hard) in [(1, 1), (1, 2), (0, 0)] {
        let root = std::env::temp_dir().join(format!("composer-model-{}", Uuid::new_v4()));
        let (backend, boss, capture, launches) = fixture(&root);
        set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", soft, hard);
        let _held = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");
        if hard > 1 {
            let _ = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");
        }
        let earlier = queued(&backend, boss, &root, "earlier", ResourceSet::default());
        let employee = queued(&backend, boss, &root, "viewed", ResourceSet::default());
        // Boss and ordinary human controls still obey admission, not the UI lane.
        for caller in [Some(boss), None] {
            backend
                .handle_boss_operation(
                    caller,
                    BossOperation::Control {
                        session_id: employee,
                        action: EmployeeControl::Prompt {
                            prompt: "normal follow-up".into(),
                            delivery: None,
                        },
                    },
                    &EventSink::detached(),
                )
                .unwrap();
        }
        backend.run_summon_scheduler();
        assert_eq!(launches.load(Ordering::SeqCst), 0);
        let submission = Uuid::new_v4();
        let prompt = "  Human line one\nHuman line two  ";
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let (backend, barrier) = (&backend, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    submit(backend, employee, submission, prompt).unwrap();
                });
            }
        });
        assert_eq!(launches.load(Ordering::SeqCst), 1);
        assert_eq!(
            backend.boss.employee_lifecycle(employee),
            Some(EmployeeLifecycle::Working)
        );
        assert_eq!(
            backend.boss.employee_lifecycle(earlier),
            Some(EmployeeLifecycle::Queued)
        );
        assert_eq!(capture.prompts.lock().len(), 1);
        assert_eq!(capture.prompts.lock()[0].matches(prompt).count(), 1);
        let ticket = backend.boss.employee(employee).unwrap().ticket.unwrap();
        assert!(ticket.human_interaction);
        assert_eq!(ticket.composer_receipts.len(), 1);
        assert!(submit(&backend, employee, submission, "different").is_err());
        // Running-turn interrupt delivery steers, without a second admission.
        backend
            .agent
            .note_driver_event(employee, &DriverEvent::TurnStarted);
        let steer_id = Uuid::new_v4();
        let steer = || {
            backend.handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id: employee,
                    runtime_id: Uuid::nil(),
                    command: Command::EmployeeComposerPrompt {
                        viewed_employee_id: employee,
                        submission_id: steer_id,
                        prompt: "Steer once".into(),
                        delivery: AgentPromptDelivery::Interrupt,
                    },
                },
                EventSink::detached(),
                None,
            )
        };
        steer().unwrap();
        steer().unwrap();
        assert_eq!(
            capture
                .steers
                .lock()
                .iter()
                .filter(|s| s.as_str() == "Steer once")
                .count(),
            1
        );
        assert_eq!(launches.load(Ordering::SeqCst), 1);
        // Its above-cap admission still consumes one ordinary model slot.
        let status = backend
            .resource_broker()
            .unwrap()
            .try_admission(
                employee,
                ticket.reservation.unwrap(),
                ResourceSet::default(),
                "viewed".into(),
                backend.admission_claim(&ticket),
            )
            .unwrap()
            .status;
        assert_eq!(
            status
                .reservations
                .iter()
                .filter(|r| r.task == employee && !r.released && !r.cancelled)
                .count(),
            1
        );
        drop(backend);
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn employee_composer_keeps_hardware_wait_and_durable_receipt_on_restart() {
    let hardware = ResourceSet {
        native_builds: 1,
        ..ResourceSet::default()
    };
    {
        let root = std::env::temp_dir().join(format!("composer-recovery-{}", Uuid::new_v4()));
        let (backend, boss, _, launches) = fixture(&root);
        set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
        let held_task = Uuid::new_v4();
        let held_id = Uuid::new_v4();
        let held_claim = waku_protocol::resources::AdmissionClaim {
            daemon: Uuid::new_v4(),
            provider: "codex".into(),
            model: "gpt-5.5".into(),
            live_limit: u32::MAX,
            hard_cap: u32::MAX,
            allow_burst: false,
        };
        assert!(
            backend
                .resource_broker()
                .unwrap()
                .try_admission(
                    held_task,
                    held_id,
                    hardware.clone(),
                    "hardware fixture".into(),
                    held_claim
                )
                .unwrap()
                .granted
        );
        let employee = queued(&backend, boss, &root, "resource-bound", hardware.clone());
        let submission = Uuid::new_v4();
        submit(&backend, employee, submission, "Do this now").unwrap();
        let ticket = backend.boss.employee(employee).unwrap().ticket.unwrap();
        assert!(ticket.human_interaction);
        assert!(
            ticket
                .blocked_by
                .iter()
                .any(|b| matches!(b, AdmissionBlocker::HostResources { .. }))
        );
        assert!(
            !ticket
                .blocked_by
                .iter()
                .any(|b| matches!(b, AdmissionBlocker::ModelLimit { .. }))
        );
        assert_eq!(launches.load(Ordering::SeqCst), 0);
        backend
            .boss
            .reticket(employee, |ticket| ticket.model = "another-pool".into())
            .unwrap();
        assert!(
            !backend
                .boss
                .employee(employee)
                .unwrap()
                .ticket
                .unwrap()
                .human_interaction
        );
        backend
            .boss
            .reticket(employee, |ticket| ticket.model = "gpt-5.5".into())
            .unwrap();
        assert!(
            !backend
                .boss
                .employee(employee)
                .unwrap()
                .ticket
                .unwrap()
                .human_interaction
        );
        submit(
            &backend,
            employee,
            Uuid::new_v4(),
            "Continue the directed turn",
        )
        .unwrap();
        let deleted = queued(&backend, boss, &root, "removed", hardware);
        submit(&backend, deleted, Uuid::new_v4(), "Remove this waiting job").unwrap();
        backend.remove_session(deleted).unwrap();
        assert_eq!(
            backend.boss.employee_lifecycle(deleted),
            Some(EmployeeLifecycle::Expired)
        );
        drop(backend);
        let mut restored = reopen_backend(&root);
        *restored.broker_root.lock() = Some(root.join("broker"));
        assert_eq!(
            restored.boss.employee_lifecycle(deleted),
            Some(EmployeeLifecycle::Expired)
        );
        assert!(
            !restored
                .task_state
                .lock()
                .sessions
                .iter()
                .any(|s| s.id == deleted)
        );
        let capture = Arc::new(CaptureDriver::default());
        let driver = capture.clone();
        restored.runtime_test_start = Some(Arc::new(move |_, _, _| {
            Ok(DriverHandle::from_control(driver.clone()))
        }));
        submit(&restored, employee, submission, "Do this now").unwrap();
        let ticket = restored.boss.employee(employee).unwrap().ticket.unwrap();
        assert_eq!(
            ticket.pending_prompts,
            ["Do this now", "Continue the directed turn"]
        );
        assert_eq!(ticket.composer_receipts.len(), 2);
        assert!(ticket.human_interaction);
        restored.run_summon_scheduler();
        assert!(capture.prompts.lock().is_empty());
        restored
            .resource_broker()
            .unwrap()
            .release_admission(held_task, held_id);
        restored.run_summon_scheduler();
        assert_eq!(capture.prompts.lock().len(), 1);
        assert_eq!(
            restored.boss.employee_lifecycle(employee),
            Some(EmployeeLifecycle::Working)
        );
        submit(&restored, employee, submission, "Do this now").unwrap();
        assert_eq!(capture.prompts.lock().len(), 1);
        // A later autonomous requeue must lose the exemption.
        restored.requeue_employee(employee, |_, _| {}).unwrap();
        assert!(
            !restored
                .boss
                .employee(employee)
                .unwrap()
                .ticket
                .unwrap()
                .human_interaction
        );
        let project = restored
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == employee)
            .unwrap()
            .project_id;
        restored.remove_project(project).unwrap();
        drop(restored);
        let recovered = reopen_backend(&root);
        *recovered.broker_root.lock() = Some(root.join("broker"));
        recovered.run_summon_scheduler();
        assert_eq!(
            recovered.boss.employee_lifecycle(employee),
            Some(EmployeeLifecycle::Expired)
        );
        assert!(
            !recovered
                .task_state
                .lock()
                .sessions
                .iter()
                .any(|session| session.id == employee)
        );
        drop(recovered);
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn employee_composer_auth_and_view_mismatch_cannot_create_intent() {
    let root = std::env::temp_dir().join(format!("composer-authority-{}", Uuid::new_v4()));
    let (backend, boss, _, launches) = fixture(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    let employee = queued(&backend, boss, &root, "viewed", ResourceSet::default());
    for (caller, viewed) in [
        (Some(boss), employee),
        (Some(employee), employee),
        (None, Uuid::new_v4()),
    ] {
        assert!(
            backend
                .handle(
                    Request {
                        request_id: Uuid::new_v4(),
                        session_id: employee,
                        runtime_id: Uuid::nil(),
                        command: Command::EmployeeComposerPrompt {
                            viewed_employee_id: viewed,
                            submission_id: Uuid::new_v4(),
                            prompt: "No".into(),
                            delivery: AgentPromptDelivery::Interrupt
                        },
                    },
                    EventSink::detached(),
                    caller
                )
                .is_err()
        );
    }
    assert!(
        !backend
            .boss
            .employee(employee)
            .unwrap()
            .ticket
            .unwrap()
            .human_interaction
    );
    assert_eq!(launches.load(Ordering::SeqCst), 0);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn employee_composer_expired_reentry_and_uncertain_forwarding_never_replay() {
    let root = std::env::temp_dir().join(format!("composer-expiry-{}", Uuid::new_v4()));
    let (backend, boss, capture, launches) = fixture(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    let employee = queued(&backend, boss, &root, "viewed", ResourceSet::default());
    let original = Uuid::new_v4();
    submit(&backend, employee, original, "First request").unwrap();
    backend
        .finish_boss_employee(
            employee,
            false,
            waku_protocol::boss::EmployeeSettle::TurnFinished,
        )
        .unwrap();
    assert_eq!(
        backend.boss.employee_lifecycle(employee),
        Some(EmployeeLifecycle::Expired)
    );
    submit(&backend, employee, original, "First request").unwrap();
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    let next = Uuid::new_v4();
    submit(&backend, employee, next, "Second request").unwrap();
    assert_eq!(
        backend.boss.employee_lifecycle(employee),
        Some(EmployeeLifecycle::Working)
    );
    assert_eq!(launches.load(Ordering::SeqCst), 2);
    assert_eq!(
        capture
            .prompts
            .lock()
            .iter()
            .filter(|p| p.contains("Second request"))
            .count(),
        1
    );
    // Crash boundary: forwarding intent is durable but acceptance was not
    // recorded. Recovery must not guess and submit a second provider effect.
    let uncertain = Uuid::new_v4();
    backend
        .boss
        .update_employee_ticket(employee, |ticket| {
            ticket
                .composer_receipts
                .push(waku_protocol::boss::EmployeeComposerReceipt {
                    submission_id: uncertain,
                    prompt: "Acceptance unknown".into(),
                    delivery: AgentPromptDelivery::Queue,
                    accepted: false,
                });
        })
        .unwrap();
    drop(backend);
    let restored = reopen_backend(&root);
    let error = submit(&restored, employee, uncertain, "Acceptance unknown").unwrap_err();
    assert!(error.to_string().contains("uncertain"));
    assert!(submit(&restored, employee, next, "Second request").is_ok());
    assert!(restored.sessions.lock().get(&employee).is_none());
    drop(restored);
    let _ = std::fs::remove_dir_all(root);
}
