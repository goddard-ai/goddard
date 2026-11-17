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
    let decision = || Action::Decide {
        id: record.id,
        expected_revision: record.revision,
        decision: CounselDecision::Send,
    };
    invoke(&backend, None, decision()).unwrap();
    let delivered = captures.lock()[&boss].prompts.lock().clone();
    assert_eq!(delivered.len(), 1);
    assert!(delivered[0].contains(&record.instruction));
    assert!(delivered[0].contains("Human-approved Counsel recommendation"));
    assert!(delivered[0].contains(&format!("goddard://task/{counsel}")));
    assert!(invoke(&backend, None, decision()).is_err());
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
                    decision: CounselDecision::Send
                }
            )
            .is_err()
        );
    }
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
                decision: CounselDecision::Send
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
    assert!(
        invoke(
            &backend,
            None,
            Action::Decide {
                id: record.id,
                expected_revision: 1,
                decision: CounselDecision::Send
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
        decision: CounselDecision::Send,
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
