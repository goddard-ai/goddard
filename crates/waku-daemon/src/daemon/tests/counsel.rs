use super::*;
use waku_protocol::counsel::{CounselDecision, CounselOperation as Action, CounselStatus};

fn fixture(
    root: &Path,
) -> (
    WakuBackend,
    Uuid,
    Arc<Mutex<HashMap<Uuid, Arc<CaptureDriver>>>>,
) {
    let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
    let mut document = settings.get();
    document.boss_experiment_enabled = true;
    settings.replace(document).unwrap();
    let (mut backend, task) = surface_test_backend(root);
    assert!(backend.boss.uses_database());
    runtime_generation_configure_launch(&mut backend, root);
    let captures = Arc::new(Mutex::new(HashMap::new()));
    let drivers = captures.clone();
    let senders = Arc::new(Mutex::new(Vec::new()));
    backend.runtime_test_start = Some(Arc::new(move |_, options, sender| {
        senders.lock().push(sender);
        let driver = Arc::new(CaptureDriver::default());
        drivers
            .lock()
            .insert(options.agent.as_ref().unwrap().task_id, driver.clone());
        Ok(DriverHandle::from_control(driver))
    }));
    (backend, task, captures)
}

fn invoke(
    backend: &WakuBackend,
    caller: Option<Uuid>,
    action: Action,
) -> anyhow::Result<BossResult> {
    backend.handle_boss_operation(
        caller,
        BossOperation::Counsel { action },
        &EventSink::detached(),
    )
}

fn open(backend: &WakuBackend) -> Uuid {
    let BossResult::Session { session, .. } = invoke(
        backend,
        None,
        Action::Open {
            provider: ProviderKind::Codex,
            model: Some("gpt-6.1-sol".into()),
            mode: Default::default(),
        },
    )
    .unwrap() else {
        panic!("expected Counsel chat")
    };
    session.id
}

fn propose(
    backend: &WakuBackend,
    counsel: Uuid,
    source: Uuid,
) -> waku_protocol::counsel::CounselRecommendation {
    let BossResult::Counsel { state } = invoke(
        backend,
        Some(counsel),
        Action::Propose {
            title: "Verify the remaining interaction".into(),
            reason: "The latest implementation result leaves the provider interaction unverified."
                .into(),
            instruction:
                "Resume the source task, verify the provider interaction, and report the result."
                    .into(),
            sources: vec![source],
            replaces: None,
        },
    )
    .unwrap() else {
        panic!("expected recommendation")
    };
    state.recommendations.last().unwrap().clone()
}

#[test]
fn counsel_owns_history_and_approved_instruction_reaches_boss_once() {
    let root = std::env::temp_dir().join(format!("counsel-handoff-{}", Uuid::new_v4()));
    let (backend, task, captures) = fixture(&root);
    let boss = rotation_boss(&backend);
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(task).unwrap();
        backend.task_store.hydrate(session).unwrap();
        session.begin_turn("Check the implemented provider interaction");
        session.push_message(
            MessageRole::Assistant,
            "Implementation checks pass; provider interaction is unverified.",
        );
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        session.archived_at = Some(1);
        backend.task_store.save(&mut state).unwrap();
    }
    let counsel = open(&backend);
    assert_ne!(counsel, boss);
    let mut settings = backend.settings.get();
    settings.agent_tools_enabled = false;
    backend.settings.replace(settings).unwrap();
    let history = backend
        .agent_history_search(
            Some(counsel),
            counsel,
            "unverified",
            None,
            None,
            None,
            None,
            None,
            None,
            0,
        )
        .unwrap();
    assert!(
        matches!(history, ResponsePayload::AgentHistorySearch { result } if result.hits.iter().any(|hit| hit.task_id == task))
    );
    assert_eq!(backend.boss.document().session_id, Some(boss));
    assert_eq!(open(&backend), counsel);
    let prompt = captures.lock()[&counsel].prompts.lock()[0].clone();
    assert!(prompt.contains("You are Counsel"));
    assert!(prompt.contains(&task.to_string()));
    assert!(prompt.contains("\"archived\":true"));
    assert!(!prompt.contains("You are employee"));
    assert!(!crate::model::strip_injected_prompt_blocks(&prompt).contains("<goddard-counsel"));
    assert!(
        backend
            .agent_read_session(Some(counsel), Some(task), None, None, None)
            .is_ok()
    );
    let record = propose(&backend, counsel, task);
    assert!(
        !captures.lock().contains_key(&boss),
        "proposing must not launch or prompt Boss"
    );
    let approve = || Action::Decide {
        id: record.id,
        expected_revision: record.revision,
        decision: CounselDecision::Approve,
    };
    let send = || Action::Send {
        id: record.id,
        expected_revision: record.revision,
    };
    // The human's button approves only: Counsel resumes to deliver, and a
    // second approval cannot bind a record that already moved on.
    invoke(&backend, None, approve()).unwrap();
    assert_eq!(
        backend.boss.document().counsel.recommendations[0].status,
        CounselStatus::Approved
    );
    assert_eq!(
        backend.boss.document().counsel.recommendations[0].approved_revision,
        Some(record.revision)
    );
    assert!(
        captures.lock()[&counsel]
            .prompts
            .lock()
            .last()
            .is_some_and(|prompt| prompt.contains("counsel send")),
        "approval resumes Counsel with the send instruction"
    );
    assert!(invoke(&backend, None, approve()).is_err());
    // Counsel carries the approval through its one authorized operation.
    invoke(&backend, Some(counsel), send()).unwrap();
    let delivered = captures.lock()[&boss].prompts.lock().clone();
    assert_eq!(delivered.len(), 1);
    assert!(delivered[0].contains(&record.instruction));
    assert!(delivered[0].contains("Human-approved Counsel recommendation"));
    assert!(delivered[0].contains(&format!("goddard://task/{counsel}")));
    // A replayed send folds onto the delivered record — no second prompt.
    invoke(&backend, Some(counsel), send()).unwrap();
    assert_eq!(captures.lock()[&boss].prompts.lock().len(), 1);
    let state = backend.boss.document().counsel;
    assert_eq!(state.recommendations[0].status, CounselStatus::Sent);
    {
        let state = backend.task_state.lock();
        let message = state
            .sessions
            .iter()
            .find(|session| session.id == boss)
            .unwrap()
            .messages
            .iter()
            .find(|message| message.id == record.id)
            .unwrap();
        assert_eq!(message.sent_by_task, Some(counsel));
        assert!(
            state
                .sessions
                .iter()
                .find(|session| session.id == counsel)
                .unwrap()
                .messages
                .iter()
                .any(|message| message.content.contains("Sent to [Boss]"))
        );
    }
    // Clearing Boss must preserve Counsel's distinct transcript and decisions.
    backend
        .handle_boss_operation(None, BossOperation::Clear, &EventSink::detached())
        .unwrap();
    assert_eq!(backend.boss.document().counsel, state);
    assert!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .any(|session| session.id == counsel)
    );
    drop(backend);
    let reloaded = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    assert_eq!(reloaded.boss.document().counsel, state);
    assert!(reloaded.boss.is_counsel(counsel));
    drop(reloaded);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn counsel_authority_cannot_approve_or_bypass_the_handoff() {
    let root = std::env::temp_dir().join(format!("counsel-authority-{}", Uuid::new_v4()));
    let (backend, task, _) = fixture(&root);
    let counsel = open(&backend);
    let record = propose(&backend, counsel, task);
    for caller in [counsel, task] {
        assert!(
            invoke(
                &backend,
                Some(caller),
                Action::Decide {
                    id: record.id,
                    expected_revision: 1,
                    decision: CounselDecision::Approve
                }
            )
            .is_err()
        );
    }
    // Delivery is Counsel's alone — the human's own client and ordinary
    // tasks cannot invoke it, and Counsel cannot send what was never
    // approved or a revision that no longer matches the approval.
    for caller in [None, Some(task)] {
        assert!(
            invoke(
                &backend,
                caller,
                Action::Send {
                    id: record.id,
                    expected_revision: 1
                }
            )
            .is_err()
        );
    }
    assert!(
        invoke(
            &backend,
            Some(counsel),
            Action::Send {
                id: record.id,
                expected_revision: 1
            }
        )
        .is_err()
    );
    assert!(invoke(&backend, Some(task), Action::View).is_err());
    assert!(
        backend
            .handle_boss_operation(
                Some(counsel),
                BossOperation::Eval {
                    script: "op(#{type: \"clear\"})".into()
                },
                &EventSink::detached()
            )
            .is_err()
    );
    assert!(
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id: counsel,
                    runtime_id: Uuid::nil(),
                    command: Command::AgentPrompt {
                        task_id: Some(task),
                        thread_id: None,
                        provider: None,
                        prompt: "do work without approval".into(),
                        delivery: AgentPromptDelivery::Queue
                    },
                },
                EventSink::detached(),
                Some(counsel)
            )
            .is_err()
    );
    assert!(
        invoke(
            &backend,
            None,
            Action::Decide {
                id: record.id,
                expected_revision: 0,
                decision: CounselDecision::Approve
            }
        )
        .is_err()
    );
    assert_eq!(
        backend.boss.document().counsel.recommendations[0].status,
        CounselStatus::Proposed
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn counsel_dismissal_survives_retries_until_source_evidence_changes() {
    let root = std::env::temp_dir().join(format!("counsel-dismiss-{}", Uuid::new_v4()));
    let (backend, task, _) = fixture(&root);
    let counsel = open(&backend);
    let record = propose(&backend, counsel, task);
    invoke(
        &backend,
        None,
        Action::Decide {
            id: record.id,
            expected_revision: 1,
            decision: CounselDecision::Later,
        },
    )
    .unwrap();
    // A stale revision — captured before Later bumped the record — cannot
    // approve what the displayed card no longer is.
    assert!(
        invoke(
            &backend,
            None,
            Action::Decide {
                id: record.id,
                expected_revision: 1,
                decision: CounselDecision::Approve
            }
        )
        .is_err()
    );
    invoke(
        &backend,
        None,
        Action::Decide {
            id: record.id,
            expected_revision: 2,
            decision: CounselDecision::Dismiss,
        },
    )
    .unwrap();
    let duplicate = propose(&backend, counsel, task);
    assert_eq!(duplicate.id, record.id);
    assert_eq!(duplicate.status, CounselStatus::Dismissed);
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(task).unwrap();
        backend.task_store.hydrate(session).unwrap();
        session.push_message(
            MessageRole::User,
            "The provider has changed; reassess the remaining check.",
        );
        session.updated_at += 1;
        backend.task_store.save(&mut state).unwrap();
    }
    let changed = propose(&backend, counsel, task);
    assert_ne!(changed.id, record.id);
    assert_eq!(changed.status, CounselStatus::Proposed);
    assert!(
        invoke(
            &backend,
            Some(counsel),
            Action::Complete {
                id: changed.id,
                expected_revision: 1,
                evidence: "not sent yet".into()
            }
        )
        .is_err()
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn counsel_failed_approval_commit_leaves_no_instruction_for_boss() {
    let root = std::env::temp_dir().join(format!("counsel-atomic-{}", Uuid::new_v4()));
    let (backend, task, captures) = fixture(&root);
    let boss = rotation_boss(&backend);
    let counsel = open(&backend);
    let record = propose(&backend, counsel, task);
    let database = rusqlite::Connection::open(root.join("app.db")).unwrap();
    database.execute_batch("CREATE TRIGGER reject_counsel_approval BEFORE UPDATE ON boss_coordinator BEGIN SELECT RAISE(ABORT, 'injected Counsel commit failure'); END;").unwrap();
    let decide = || Action::Decide {
        id: record.id,
        expected_revision: 1,
        decision: CounselDecision::Approve,
    };
    assert!(invoke(&backend, None, decide()).is_err());
    assert_eq!(
        backend.boss.document().counsel.recommendations[0].status,
        CounselStatus::Proposed
    );
    assert!(!captures.lock().contains_key(&boss));
    {
        let state = backend.task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == boss)
            .unwrap();
        assert!(
            !session
                .queued_messages
                .iter()
                .any(|message| message.id == record.id)
        );
        assert!(
            !session
                .messages
                .iter()
                .any(|message| message.id == record.id)
        );
    }
    database
        .execute_batch("DROP TRIGGER reject_counsel_approval;")
        .unwrap();
    invoke(&backend, None, decide()).unwrap();
    invoke(
        &backend,
        Some(counsel),
        Action::Send {
            id: record.id,
            expected_revision: 1,
        },
    )
    .unwrap();
    assert_eq!(captures.lock()[&boss].prompts.lock().len(), 1);
    drop(database);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn counsel_reopens_the_saved_review_after_a_provider_launch_failure() {
    let root = std::env::temp_dir().join(format!("counsel-retry-{}", Uuid::new_v4()));
    let (mut backend, task, captures) = fixture(&root);
    let start = backend.runtime_test_start.take().unwrap();
    let fail = std::sync::atomic::AtomicBool::new(true);
    backend.runtime_test_start = Some(Arc::new(move |provider, options, sender| {
        if fail.swap(false, std::sync::atomic::Ordering::SeqCst) {
            anyhow::bail!("injected provider launch failure");
        }
        start(provider, options, sender)
    }));
    let result = invoke(
        &backend,
        None,
        Action::Open {
            provider: ProviderKind::Codex,
            model: None,
            mode: Default::default(),
        },
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("queued review could not start")
    );
    let counsel = backend.boss.document().counsel.session_id.unwrap();
    let review_id = {
        let mut state = backend.task_state.lock();
        let source = state.session_mut(task).unwrap();
        source.updated_at += 1;
        backend.task_store.save(&mut state).unwrap();
        state
            .sessions
            .iter()
            .find(|session| session.id == counsel)
            .unwrap()
            .queued_messages[0]
            .id
    };
    assert_eq!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == counsel)
            .unwrap()
            .queued_messages
            .len(),
        1
    );
    assert_eq!(open(&backend), counsel);
    assert_eq!(captures.lock()[&counsel].prompts.lock().len(), 1);
    assert!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == counsel)
            .unwrap()
            .messages
            .iter()
            .any(|message| message.id == review_id)
    );
    assert!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == counsel)
            .unwrap()
            .queued_messages
            .is_empty()
    );
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn settled_counsel_answer_claims_one_recommendation_turn() {
    let root = std::env::temp_dir().join(format!("counsel-settle-{}", Uuid::new_v4()));
    let (backend, _task, _captures) = fixture(&root);
    let counsel = open(&backend);
    // A human question's completed turn is the only eligible candidate —
    // employee and generated turns never reach this point.
    let source_turn = {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(counsel).unwrap();
        backend.task_store.hydrate(session).unwrap();
        let turn = session.begin_turn("What should I look at next?");
        session.push_message(MessageRole::Assistant, "Check the audit task.");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        backend.task_store.save(&mut state).unwrap();
        turn
    };
    let (candidate, _) = backend.counsel_recommendation_candidate(counsel).unwrap();
    assert_eq!(candidate, source_turn);
    assert!(backend.claim_counsel_recommendation_turn(
        counsel,
        source_turn,
        &EventSink::detached()
    ));
    // Same source turn can never claim twice — replayed settles and
    // restarts see it recorded.
    assert!(!backend.claim_counsel_recommendation_turn(
        counsel,
        source_turn,
        &EventSink::detached()
    ));
    assert!(backend.counsel_recommendation_candidate(counsel).is_none());
    assert_eq!(
        backend.boss.document().counsel.recommendation_turns,
        vec![source_turn]
    );
    // The generated recommendation turn itself is not human-authored — it
    // can never recursively wake another recommendation turn.
    let second_turn = {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(counsel).unwrap();
        let turn = session.begin_hidden_turn("recommend something");
        session.push_message(MessageRole::Assistant, "Proposing a step.");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        backend.task_store.save(&mut state).unwrap();
        turn
    };
    assert!(backend.counsel_recommendation_candidate(counsel).is_none());
    // A newer human turn settles later: the stale verdict for the hidden
    // turn cannot claim it, and the new turn claims once.
    assert!(!backend.claim_counsel_recommendation_turn(
        counsel,
        second_turn,
        &EventSink::detached()
    ));
    let third_turn = {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(counsel).unwrap();
        let turn = session.begin_turn("And after that?");
        session.push_message(MessageRole::Assistant, "Then review the send.");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        backend.task_store.save(&mut state).unwrap();
        turn
    };
    // Intervening input voided the second turn's window; the latest one is
    // what a verdict may claim.
    assert!(!backend.claim_counsel_recommendation_turn(
        counsel,
        second_turn,
        &EventSink::detached()
    ));
    assert!(backend.claim_counsel_recommendation_turn(
        counsel,
        third_turn,
        &EventSink::detached()
    ));
    // A user stop parks the turn's claim: a stopped Counsel is not woken.
    let fourth = {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(counsel).unwrap();
        let turn = session.begin_turn("Anything else?");
        session.push_message(MessageRole::Assistant, "No clear next step.");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        backend.task_store.save(&mut state).unwrap();
        turn
    };
    backend.agent.note_user_stop(counsel);
    assert!(backend.counsel_recommendation_candidate(counsel).is_none());
    assert!(!backend.claim_counsel_recommendation_turn(counsel, fourth, &EventSink::detached()));
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn non_counsel_sessions_never_claim_recommendation_turns() {
    let root = std::env::temp_dir().join(format!("counsel-scope-{}", Uuid::new_v4()));
    let (backend, task, _captures) = fixture(&root);
    let _counsel = open(&backend);
    let source_turn = {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(task).unwrap();
        backend.task_store.hydrate(session).unwrap();
        let turn = session.begin_turn("Finish the audit");
        session.push_message(MessageRole::Assistant, "Done — all checks pass.");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        backend.task_store.save(&mut state).unwrap();
        turn
    };
    // Another task's settled answer is not Counsel's to evaluate.
    assert!(backend.counsel_recommendation_candidate(task).is_none());
    assert!(!backend.claim_counsel_recommendation_turn(task, source_turn, &EventSink::detached()));
    assert!(backend.boss.document().counsel.recommendation_turns.is_empty());
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn counsel_send_failure_is_reported_and_retriable() {
    let root = std::env::temp_dir().join(format!("counsel-sendfail-{}", Uuid::new_v4()));
    let (mut backend, task, _captures) = fixture(&root);
    let boss = rotation_boss(&backend);
    // First launch serves Counsel; the injected failure hits Boss's launch.
    let start = backend.runtime_test_start.take().unwrap();
    backend.runtime_test_start = Some(Arc::new(move |provider, options, sender| {
        if options
            .agent
            .as_ref()
            .is_some_and(|agent| agent.task_id == boss)
        {
            anyhow::bail!("injected Boss launch failure");
        }
        start(provider, options, sender)
    }));
    let counsel = open(&backend);
    let record = propose(&backend, counsel, task);
    invoke(
        &backend,
        None,
        Action::Decide {
            id: record.id,
            expected_revision: 1,
            decision: CounselDecision::Approve,
        },
    )
    .unwrap();
    assert!(
        invoke(
            &backend,
            Some(counsel),
            Action::Send {
                id: record.id,
                expected_revision: 1,
            }
        )
        .is_err()
    );
    let record = &backend.boss.document().counsel.recommendations[0];
    assert_eq!(record.status, CounselStatus::DeliveryFailed);
    assert!(
        record
            .delivery_error
            .as_deref()
            .is_some_and(|error| error.contains("injected Boss launch failure")),
        "the durable record names the real failure: {:?}",
        record.delivery_error
    );
    // The parked mirror survives — a retry is a fresh attempt on the same
    // approved revision, not a lost approval.
    let state = backend.task_state.lock();
    assert!(
        state
            .sessions
            .iter()
            .find(|session| session.id == boss)
            .unwrap()
            .queued_messages
            .iter()
            .any(|message| message.id == record.id)
    );
    drop(state);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn counsel_propose_anchors_the_card_to_its_originating_message() {
    let root = std::env::temp_dir().join(format!("counsel-anchor-{}", Uuid::new_v4()));
    let (backend, task, _captures) = fixture(&root);
    let counsel = open(&backend);
    let anchor = {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(counsel).unwrap();
        backend.task_store.hydrate(session).unwrap();
        session.begin_turn("what is next");
        session.push_message(MessageRole::Assistant, "Here is my read.");
        session.finish_active_turn(crate::model::TurnStatus::Completed);
        let anchor = session.messages.last().unwrap().id;
        backend.task_store.save(&mut state).unwrap();
        anchor
    };
    let record = propose(&backend, counsel, task);
    assert_eq!(record.anchor, Some(anchor));
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

fn human_turn(backend: &WakuBackend, session_id: Uuid, prompt: &str, completed: bool) -> Uuid {
    let mut state = backend.task_state.lock();
    let session = state.session_mut(session_id).unwrap();
    backend.task_store.hydrate(session).unwrap();
    session.finish_active_turn(TurnStatus::Completed);
    let turn = session.begin_turn(prompt);
    if completed {
        session.finish_active_turn(TurnStatus::Completed);
    }
    backend.task_store.save(&mut state).unwrap();
    turn
}

#[test]
fn direct_counsel_handoff_steers_busy_boss_and_requires_human_turn() {
    let root = std::env::temp_dir().join(format!("counsel-direct-{}", Uuid::new_v4()));
    let (backend, task, captures) = fixture(&root);
    let boss = rotation_boss(&backend);
    let counsel = open(&backend);
    let recommendation = propose(&backend, counsel, task);
    let instruction = "Check the build and report only the failing check.";
    let handoff = || Action::Handoff {
        instruction: instruction.into(),
    };
    assert!(invoke(&backend, Some(counsel), handoff()).is_err());

    human_turn(
        &backend,
        counsel,
        &format!("Tell Boss: {instruction}"),
        false,
    );
    assert!(invoke(&backend, Some(task), handoff()).is_err());

    backend
        .ensure_agent_runtime(boss, &EventSink::detached())
        .unwrap();
    backend
        .agent
        .note_driver_event(boss, &DriverEvent::TurnStarted);
    assert!(matches!(
        invoke(&backend, Some(counsel), handoff()).unwrap(),
        BossResult::Saved
    ));
    let captures = captures.lock();
    let steers = captures[&boss].steers.lock();
    assert_eq!(steers.len(), 1);
    assert!(steers[0].contains(&format!("task id {counsel}")));
    assert!(steers[0].ends_with(instruction));
    assert!(captures[&boss].prompts.lock().is_empty());
    let state = backend.boss.document().counsel;
    assert_eq!(state.recommendations, vec![recommendation]);
    drop(steers);
    drop(captures);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_counsel_handoff_reports_launch_failure() {
    let root = std::env::temp_dir().join(format!("counsel-direct-{}", Uuid::new_v4()));
    let (mut backend, _, _) = fixture(&root);
    let boss = rotation_boss(&backend);
    let start = backend.runtime_test_start.take().unwrap();
    backend.runtime_test_start = Some(Arc::new(move |provider, options, sender| {
        if options
            .agent
            .as_ref()
            .is_some_and(|agent| agent.task_id == boss)
        {
            anyhow::bail!("injected Boss launch failure");
        }
        start(provider, options, sender)
    }));
    let counsel = open(&backend);
    human_turn(&backend, counsel, "Tell Boss to check the build.", false);
    let error = invoke(
        &backend,
        Some(counsel),
        Action::Handoff {
            instruction: "Check the build.".into(),
        },
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("injected Boss launch failure"));
    assert!(backend.boss.document().counsel.recommendations.is_empty());
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn counsel_messages_only_existing_normal_tasks_through_the_scoped_rpc() {
    let root = std::env::temp_dir().join(format!("counsel-direct-{}", Uuid::new_v4()));
    let (backend, task, captures) = fixture(&root);
    assert!(!backend.settings.get().agent_tools_enabled);
    let boss = rotation_boss(&backend);
    let counsel = open(&backend);
    let send = |target, prompt: &str, delivery| {
        backend.handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: counsel,
                runtime_id: Uuid::nil(),
                command: Command::AgentPrompt {
                    task_id: Some(target),
                    thread_id: None,
                    provider: None,
                    prompt: prompt.into(),
                    delivery,
                },
            },
            EventSink::detached(),
            Some(counsel),
        )
    };
    assert!(send(task, "Check the build.", AgentPromptDelivery::Queue).is_err());
    human_turn(
        &backend,
        counsel,
        "Tell the task to check the build.",
        false,
    );
    for target in [counsel, boss, Uuid::new_v4()] {
        assert!(send(target, "Check the build.", AgentPromptDelivery::Interrupt).is_err());
    }
    let special = {
        let mut state = backend.task_state.lock();
        let project = state
            .sessions
            .iter()
            .find(|session| session.id == task)
            .unwrap()
            .project_id;
        let mut side_chat = AgentSession::new(project, ProviderKind::Codex);
        side_chat.side_chat_of = Some(task);
        let id = side_chat.id;
        state.push_session(side_chat);
        id
    };
    assert!(send(special, "Check the build.", AgentPromptDelivery::Queue).is_err());
    send(task, "Check the build.", AgentPromptDelivery::Queue).unwrap();
    {
        let state = backend.task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == task)
            .unwrap();
        assert!(session.messages.iter().any(|message| {
            message.content == "Check the build." && message.sent_by_task == Some(counsel)
        }));
    }
    backend
        .agent
        .note_driver_event(task, &DriverEvent::TurnStarted);
    send(
        task,
        "Include the test count.",
        AgentPromptDelivery::Interrupt,
    )
    .unwrap();
    assert!(captures.lock()[&task].steers.lock().iter().any(|steer| {
        steer.contains(&format!("task id {counsel}")) && steer.ends_with("Include the test count.")
    }));
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn outstanding_recommendations_block_scheduling_and_parked_delivery() {
    let root = std::env::temp_dir().join(format!("counsel-direct-{}", Uuid::new_v4()));
    let (backend, task, captures) = fixture(&root);
    let counsel = open(&backend);
    let source = human_turn(&backend, counsel, "What remains?", true);
    assert!(backend.counsel_recommendation_candidate(counsel).is_some());
    // Keep a scheduled recommendation parked while a proposal wins the race.
    backend
        .agent
        .note_driver_event(counsel, &DriverEvent::TurnStarted);
    assert!(backend.claim_counsel_recommendation_turn(counsel, source, &EventSink::detached()));
    let proposed = propose(&backend, counsel, task);
    invoke(
        &backend,
        None,
        Action::Decide {
            id: proposed.id,
            expected_revision: proposed.revision,
            decision: CounselDecision::Later,
        },
    )
    .unwrap();
    let latest = human_turn(
        &backend,
        counsel,
        "Tell me more about the earlier proposal.",
        true,
    );
    assert_eq!(
        backend.boss.document().counsel.recommendations[0].status,
        CounselStatus::Later
    );
    assert!(backend.counsel_recommendation_candidate(counsel).is_none());
    assert!(!backend.claim_counsel_recommendation_turn(counsel, latest, &EventSink::detached()));
    backend.agent.note_driver_event(
        counsel,
        &DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None,
        },
    );
    let count = captures.lock()[&counsel].prompts.lock().len();
    open(&backend);
    assert_eq!(captures.lock()[&counsel].prompts.lock().len(), count);
    let state = backend.task_state.lock();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == counsel)
        .unwrap();
    assert!(session.queued_messages.is_empty());
    drop(state);
    drop(backend);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn counsel_revision_preserves_both_anchors_on_reopen() {
    let root = std::env::temp_dir().join(format!("counsel-revision-{}", Uuid::new_v4()));
    let (backend, task, _) = fixture(&root);
    let counsel = open(&backend);
    human_turn(&backend, counsel, "What should I do next?", true);
    let first = propose(&backend, counsel, task);
    invoke(
        &backend,
        None,
        Action::Decide {
            id: first.id,
            expected_revision: first.revision,
            decision: CounselDecision::Later,
        },
    )
    .unwrap();
    human_turn(&backend, counsel, "Revise that to cover only Codex.", false);
    let BossResult::Counsel { state } = invoke(
        &backend,
        Some(counsel),
        Action::Propose {
            title: "Verify Codex".into(),
            reason: "Narrow the verification to the requested provider.".into(),
            instruction: "Verify the Codex interaction only.".into(),
            sources: vec![task],
            replaces: Some(first.id),
        },
    )
    .unwrap() else {
        panic!("expected revised Counsel state");
    };
    assert_eq!(state.recommendations[0].anchor, first.anchor);
    assert_eq!(state.recommendations[0].status, CounselStatus::Dismissed);
    assert!(state.recommendations[1].anchor.is_some());
    assert_ne!(state.recommendations[1].anchor, first.anchor);
    assert_eq!(state.recommendations[1].status, CounselStatus::Proposed);

    drop(backend);
    let reloaded = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    assert_eq!(reloaded.boss.document().counsel, state);
    let mut persisted = reloaded.task_state.lock();
    let session = persisted.session_mut(counsel).unwrap();
    reloaded.task_store.hydrate(session).unwrap();
    for record in &state.recommendations {
        assert!(
            session
                .messages
                .iter()
                .any(|message| Some(message.id) == record.anchor)
        );
    }
    drop(persisted);
    drop(reloaded);
    let _ = std::fs::remove_dir_all(root);
}
