use super::*;
use std::os::unix::fs::PermissionsExt as _;

#[test]
fn rewind_to_beginning_keeps_the_task_and_options_after_reload() {
    let root = std::env::temp_dir().join(format!("rewind-persistence-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let repo = root.join("repo");
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
            ])
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet"]);
    std::fs::write(repo.join("fixture.txt"), "before the first message\n").unwrap();
    git(&["add", "fixture.txt"]);
    git(&["commit", "--quiet", "-m", "fixture"]);
    crate::checkpoint::capture_ref(&repo, &crate::checkpoint::turn_start_ref(session_id, 1))
        .unwrap();
    std::fs::write(
        repo.join("fixture.txt"),
        "unsaved changes after the message\n",
    )
    .unwrap();

    let binary = root.join("claude");
    std::fs::write(&binary, "#!/bin/sh\necho '2.0.0 (Claude Code)'\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut settings = backend.settings.get();
    settings
        .provider_binary_overrides
        .insert(ProviderKind::Claude, binary.to_string_lossy().into_owned());
    backend.settings.replace(settings).unwrap();
    {
        let mut state = backend.task_state.lock();
        let source = state
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .unwrap();
        backend.task_store.hydrate(source).unwrap();
        source.provider = ProviderKind::Claude;
        source.provider_cursor = Some(ProviderResumeCursor::Claude {
            session_id: Uuid::new_v4().to_string(),
            resume_at: None,
        });
        source.provider_session_id = Some("legacy-history-must-not-return".into());
        source.title = "Keep this task".into();
        source.model = Some("claude-sonnet-4-6".into());
        source.reasoning_effort = Some("medium".into());
        source.turns[0].provider_turn_started = true;
        state.mark_session_dirty(session_id);
        backend.task_store.save(&mut state).unwrap();
    }
    let (rewound, _) = backend
        .rewind_session_to_message(
            session_id,
            1,
            Uuid::nil(),
            &crate::server::RuntimeCancellation::default(),
        )
        .unwrap();
    assert!(rewound.turns.is_empty() && rewound.provider_cursor.is_none());
    assert!(rewound.provider_session_id.is_none());
    let files = std::fs::read_to_string(repo.join("fixture.txt")).unwrap();
    drop(backend);
    let reloaded_store = StateStore::daemon(root.join("app.db"));
    let restored = reloaded_store.load().unwrap();
    let mut task = restored.sessions.into_iter().find(|s| s.id == session_id);
    if let Some(task) = task.as_mut() {
        reloaded_store.hydrate(task).unwrap();
    }
    drop(reloaded_store);
    let _ = std::fs::remove_dir_all(&root);

    let task = task.expect("a completed rewind must not turn the task into a discarded draft");
    assert_eq!(task.title, "Keep this task");
    assert_eq!(task.provider, ProviderKind::Claude);
    assert_eq!(task.model.as_deref(), Some("claude-sonnet-4-6"));
    assert_eq!(task.reasoning_effort.as_deref(), Some("medium"));
    assert!(task.turns.is_empty() && task.provider_cursor.is_none());
    assert!(!task.provider_locked());
    assert!(!task.messages.is_empty());
    assert!(
        task.messages
            .iter()
            .all(|m| m.role == crate::model::MessageRole::System)
    );
    assert_eq!(files, "before the first message\n");
}
