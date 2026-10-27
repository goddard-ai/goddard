use super::*;
use std::io::Write;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

struct OwnedChild(Child);
impl OwnedChild {
    fn finish(&mut self) {
        self.0.stdin.take().unwrap().write_all(b"finish\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "controlled provider did not stop"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn options(cwd: &Path) -> DriverStartOptions {
    DriverStartOptions {
        binary: "unused-controlled-factory".into(),
        cwd: cwd.to_owned(),
        mode: crate::model::RuntimeMode::FullAccess,
        model: None,
        reasoning_effort: None,
        service_tier: None,
        context_window: None,
        agent_preset: None,
        computer_use_enabled: false,
        computer_use_runtime: None,
        agent: None,
        read_own_transcript: false,
        subagents: None,
        mcp_servers: Vec::new(),
        http_mcp_capability_recorder: None,
        provider_cursor: None,
        eval: None,
        sandbox: None,
        allow_model_fallback: false,
        ephemeral: false,
    }
}

#[test]
fn native_writer_origins_precede_factory_contact_survive_reopen_and_fence_reused_runtime_ids() {
    let root = std::env::temp_dir().join(format!("native-writer-recovery-{}", Uuid::new_v4()));
    let (mut backend, session_id) = super::super::tests::surface_test_backend(&root);
    let workspace = root.join("repo");
    let unrelated = root.join("unrelated");
    std::fs::create_dir_all(&unrelated).unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let children = Arc::new(Mutex::new(Vec::<OwnedChild>::new()));
    let witnesses = Arc::new(Mutex::new(Vec::<driver::WriterCompletion>::new()));
    let store = Arc::clone(&backend.task_store);
    let called = Arc::clone(&attempts);
    let owned = Arc::clone(&children);
    let proofs = Arc::clone(&witnesses);
    backend.runtime_test_start = Some(Arc::new(move |_, options, sender| {
        let index = called.fetch_add(1, Ordering::Relaxed);
        let origins = store.native_writer_origins().unwrap();
        assert_eq!(
            origins.len(),
            index + 1,
            "factory contacted before durable admission"
        );
        let witness = sender.writer_completion();
        witness.register();
        let child = ProcessCommand::new("/bin/sh")
            .args([
                "-c",
                "printf live > \"$1\"; IFS= read -r release; printf stopped >> \"$1\"",
                "fixture",
            ])
            .arg(options.cwd.join(format!("provider-{index}")))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        owned.lock().push(OwnedChild(child));
        proofs.lock().push(witness);
        sender.send(DriverEvent::ProcessExited).unwrap();
        anyhow::bail!("controlled factory failed after creating a writer")
    }));
    // A failed durable admission cannot enter even a factory which might spawn.
    let database = backend.task_store.path().to_owned();
    let aside = root.join("held.db");
    std::fs::rename(&database, &aside).unwrap();
    std::fs::create_dir(&database).unwrap();
    let runtime_id = Uuid::new_v4();
    let (sender, _received) = driver::test_event_channel();
    assert!(
        backend
            .start_runtime_driver(
                session_id,
                runtime_id,
                ProviderKind::Claude,
                options(&workspace),
                sender,
                false
            )
            .is_err()
    );
    assert_eq!(attempts.load(Ordering::Relaxed), 0);
    std::fs::remove_dir(&database).unwrap();
    std::fs::rename(&aside, &database).unwrap();

    for _ in 0..2 {
        let (sender, received) = driver::test_event_channel();
        assert!(
            backend
                .start_runtime_driver(
                    session_id,
                    runtime_id,
                    ProviderKind::Claude,
                    options(&workspace),
                    sender,
                    false
                )
                .is_err()
        );
        assert!(matches!(
            received.recv().unwrap(),
            DriverEvent::ProcessExited
        ));
    }
    assert_eq!(backend.task_store.native_writer_origins().unwrap().len(), 2);
    let replacement = WakuBackend::new(
        DaemonSettingsStore::open(root.join("replacement-settings.json")).unwrap(),
        StateStore::daemon(database.clone()),
    )
    .unwrap();
    assert!(
        replacement
            .require_no_observed_workspace_writers(&workspace)
            .is_err()
    );
    replacement
        .require_no_observed_workspace_writers(&unrelated)
        .unwrap();
    assert!(
        children
            .lock()
            .iter_mut()
            .all(|child| child.0.try_wait().unwrap().is_none())
    );

    children.lock()[0].finish();
    // Losing a completion write preserves the origin and a bounded retry receipt.
    std::fs::rename(&database, &aside).unwrap();
    std::fs::create_dir(&database).unwrap();
    witnesses.lock()[0].confirm();
    assert_eq!(
        backend.runtime_lifecycles.native_confirmations.lock().len(),
        1
    );
    std::fs::remove_dir(&database).unwrap();
    std::fs::rename(&aside, &database).unwrap();
    assert!(
        backend
            .require_no_observed_workspace_writers(&workspace)
            .is_err()
    );
    assert!(
        backend
            .runtime_lifecycles
            .native_confirmations
            .lock()
            .is_empty()
    );
    assert_eq!(
        backend.task_store.native_writer_origins().unwrap().len(),
        1,
        "old completion released the replacement attempt"
    );
    assert!(
        replacement
            .require_no_observed_workspace_writers(&workspace)
            .is_err()
    );

    children.lock()[1].finish();
    witnesses.lock()[1].confirm();
    assert!(
        replacement
            .task_store
            .native_writer_origins()
            .unwrap()
            .is_empty()
    );
    replacement
        .require_no_observed_workspace_writers(&workspace)
        .unwrap();
    for index in 0..2 {
        assert_eq!(
            std::fs::read(workspace.join(format!("provider-{index}"))).unwrap(),
            b"livestopped"
        );
    }
    drop(children);
    drop(witnesses);
    drop(replacement);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}
