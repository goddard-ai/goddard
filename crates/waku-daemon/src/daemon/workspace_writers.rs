//! Observe retired provider writers without retaining their driver handles.
//! This is a necessary cleanup precondition, not a filesystem-removal permit.
//! Archive authority, shared owners, other writers, and admission still need
//! their own checks at the final destructive boundary.

use super::*;
use driver::{WriterCompletion, WriterCompletionState};

#[derive(Default)]
pub(super) struct RetiredWriters {
    pending: Vec<(PathBuf, WriterCompletion)>,
    // An unqualified transport leaves one conservative marker per workspace,
    // rather than retaining a driver or accumulating one marker per restart.
    unknown: HashSet<PathBuf>,
}

/// Keep attempted launches visible before a RuntimeEntry exists. A failed
/// factory call can already have contacted a provider; absence of an entry
/// therefore cannot release its unconfirmed writer observation.
pub(super) struct StartingWriter {
    lifecycles: Arc<RuntimeLifecycles>,
    witness: WriterCompletion,
}

impl StartingWriter {
    /// Stable attachments share the physical host generation's witness. Swap
    /// the provisional launch observation without a gap in writer accounting.
    pub(super) fn bind(&mut self, witness: WriterCompletion) {
        if witness.state() == WriterCompletionState::Unknown {
            return;
        }
        let mut writers = self.lifecycles.retired_writers.lock();
        let mut paths = Vec::new();
        writers.pending.retain(|(path, known)| {
            if known.same_writer(&self.witness) {
                paths.push(path.clone());
                false
            } else {
                true
            }
        });
        for path in paths {
            if !writers
                .pending
                .iter()
                .any(|(known_path, known)| *known_path == path && known.same_writer(&witness))
            {
                writers.pending.push((path, witness.clone()));
            }
        }
        self.witness = witness;
    }
}

impl Drop for StartingWriter {
    fn drop(&mut self) {
        if self.witness.state() == WriterCompletionState::Unknown {
            let mut writers = self.lifecycles.retired_writers.lock();
            let mut unknown = Vec::new();
            writers.pending.retain(|(path, witness)| {
                if witness.same_writer(&self.witness) {
                    unknown.push(path.clone());
                    false
                } else {
                    true
                }
            });
            writers.unknown.extend(unknown);
        }
    }
}

impl RuntimeLifecycles {
    pub(super) fn observe_starting_writer(
        self: &Arc<Self>,
        cwd: &Path,
        witness: WriterCompletion,
    ) -> StartingWriter {
        let mut writers = self.retired_writers.lock();
        writers
            .pending
            .retain(|(_, witness)| witness.state() != WriterCompletionState::Confirmed);
        writers.pending.push((cwd.to_owned(), witness.clone()));
        StartingWriter {
            lifecycles: Arc::clone(self),
            witness,
        }
    }

    /// Call while holding the runtime map lock, before removal becomes visible.
    /// A witness is unique to its launch: an older completion cannot release a
    /// replacement writer's witness, even when the caller reuses a runtime id.
    pub(super) fn remember_retired_writer(&self, entry: &RuntimeEntry) {
        let witness = entry.driver.writer_completion();
        let mut retired = self.retired_writers.lock();
        retired
            .pending
            .retain(|(_, witness)| witness.state() != WriterCompletionState::Confirmed);
        match witness.state() {
            WriterCompletionState::Confirmed => {}
            WriterCompletionState::Pending => {
                if !retired
                    .pending
                    .iter()
                    .any(|(_, known)| known.same_writer(&witness))
                {
                    retired.pending.push((entry.cwd.clone(), witness));
                }
            }
            WriterCompletionState::Unknown => {
                retired.unknown.insert(entry.cwd.clone());
            }
        }
    }

    fn unfinished_writer_paths(&self) -> Vec<PathBuf> {
        let mut retired = self.retired_writers.lock();
        retired
            .pending
            .retain(|(_, witness)| witness.state() != WriterCompletionState::Confirmed);
        retired
            .unknown
            .iter()
            .cloned()
            .chain(retired.pending.iter().map(|(path, _)| path.clone()))
            .collect()
    }
}

impl WakuBackend {
    /// Worker-only: normalize aliases outside the runtime locks, then reject
    /// removal while an observed provider is live or its stop is unconfirmed.
    /// Successful return does not authorize deletion or exclude a new launch;
    /// the retirement executor must also hold its workspace admission fence.
    pub(super) fn require_no_observed_workspace_writers(&self, path: &Path) -> anyhow::Result<()> {
        // Removal registers retired evidence before releasing this same map
        // lock. Looking at active first and retired second cannot miss the
        // transition between those two registries.
        let mut paths = self
            .sessions
            .lock()
            .values()
            .map(|entry| entry.cwd.clone())
            .collect::<Vec<_>>();
        paths.extend(self.runtime_lifecycles.unfinished_writer_paths());
        let target = dunce::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        for cwd in paths {
            let cwd = dunce::canonicalize(&cwd).unwrap_or(cwd);
            anyhow::ensure!(
                !(cwd.starts_with(&target) || target.starts_with(&cwd)),
                "workspace cleanup deferred: a provider writer is still active or its stop is unconfirmed"
            );
        }
        crate::stable_runtime::require_quiet_workspace_writers(self, &target)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::Duration;

    fn reap(receiver: &crossbeam_channel::Receiver<DriverEvent>) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match receiver
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                Ok(DriverEvent::ProcessExited) => return,
                Ok(DriverEvent::Error(error)) => panic!("controlled provider failed: {error}"),
                Ok(_) => {}
                Err(error) => panic!("controlled provider did not exit: {error}"),
            }
        }
    }

    #[test]
    fn retired_writer_witnesses_fence_real_worktree_removal_across_runtime_id_reuse() {
        let root = std::env::temp_dir().join(format!("workspace-writers-{}", Uuid::new_v4()));
        let (backend, session_id) = super::super::tests::surface_test_backend(&root);
        let repo = root.join("repo");
        let worktree = root.join("checkout");
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ]);
        git(&["worktree", "add", "--detach", worktree.to_str().unwrap()]);
        // A failed assertion must still let the controlled child processes
        // finish; their stdout drains intentionally wait for these markers.
        struct ReleaseWriters(PathBuf);
        impl Drop for ReleaseWriters {
            fn drop(&mut self) {
                for name in ["first", "second"] {
                    let _ = std::fs::write(self.0.join(format!("release-{name}")), "");
                }
            }
        }
        let _release_writers = ReleaseWriters(worktree.clone());
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&worktree, &alias).unwrap();
        let runtime_id = Uuid::new_v4();
        let start = |name: &str| {
            let binary = root.join(name);
            std::fs::write(
                &binary,
                r#"#!/usr/bin/env python3
import pathlib, sys, time
name = pathlib.Path(sys.argv[0]).name
for line in sys.stdin:
    pass
pathlib.Path('stopping-' + name).touch()
while not pathlib.Path('release-' + name).exists():
    time.sleep(0.005)
"#,
            )
            .unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
            let (events, receiver) = driver::test_event_channel();
            let starting = backend
                .runtime_lifecycles
                .observe_starting_writer(&worktree, events.writer_completion());
            assert!(
                backend
                    .require_no_observed_workspace_writers(&alias)
                    .is_err(),
                "in-flight launch was invisible"
            );
            let handle = driver::start_local(
                ProviderKind::Claude,
                DriverStartOptions {
                    binary,
                    cwd: worktree.clone(),
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
                },
                events,
            )
            .unwrap();
            backend.sessions.lock().insert(
                session_id,
                RuntimeEntry {
                    plan_gate: None,
                    runtime_id,
                    driver: handle.clone(),
                    stable: None,
                    last_active: std::time::Instant::now(),
                    resumable: true,
                    computer_use_available: false,
                    provider: ProviderKind::Claude,
                    cwd: worktree.clone(),
                },
            );
            drop(starting);
            (handle, receiver)
        };
        let (first, first_events) = start("first");
        drop(backend.remove_runtime(session_id, Some(runtime_id), false));
        assert!(backend.sessions.lock().is_empty());
        assert!(
            backend
                .require_no_observed_workspace_writers(&alias)
                .is_err()
        );
        // Deliberately reuse the caller's runtime id. The independent witness
        // must keep a later writer fenced when the first finally confirms.
        let (second, second_events) = start("second");
        drop(backend.remove_runtime(session_id, Some(runtime_id), false));
        std::fs::write(worktree.join("release-first"), "").unwrap();
        reap(&first_events);
        assert_eq!(
            first.writer_completion().state(),
            WriterCompletionState::Confirmed
        );
        assert_eq!(
            second.writer_completion().state(),
            WriterCompletionState::Pending
        );
        let capture = |git_ref: String| {
            backend.handle(
                waku_protocol::Request {
                    request_id: Uuid::new_v4(),
                    session_id: Uuid::nil(),
                    runtime_id: Uuid::nil(),
                    command: Command::Workspace {
                        operation: WorkspaceOperation::CaptureRef {
                            cwd: worktree.clone(),
                            git_ref,
                        },
                    },
                },
                EventSink::detached(),
                None,
            )
        };
        assert!(
            capture(waku_git::checkpoint::archive_ref(session_id)).is_err(),
            "archive snapshot raced an unfinished writer"
        );
        capture(waku_git::checkpoint::checkpoint_ref(session_id, 0)).unwrap();
        let remove = || {
            backend.handle(
                waku_protocol::Request {
                    request_id: Uuid::new_v4(),
                    session_id: Uuid::nil(),
                    runtime_id: Uuid::nil(),
                    command: Command::Workspace {
                        operation: WorkspaceOperation::RemoveWorktree {
                            path: alias.clone(),
                            force: true,
                        },
                    },
                },
                EventSink::detached(),
                None,
            )
        };
        let error = remove().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("writer is still active or its stop is unconfirmed")
        );
        assert!(
            worktree.is_dir(),
            "unconfirmed writer's checkout was removed"
        );
        std::fs::write(worktree.join("release-second"), "").unwrap();
        reap(&second_events);
        assert!(
            backend
                .require_no_observed_workspace_writers(&alias)
                .is_ok()
        );
        capture(waku_git::checkpoint::archive_ref(session_id)).unwrap();
        remove().unwrap();
        assert!(!worktree.exists());
        drop(first);
        drop(second);
        drop(backend);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stable_writer_survives_detach_shares_reattach_and_fences_replacement() {
        use waku_runtime::{
            AdapterFactory, AdapterRegistry, FakeFactory, HostOptions, RuntimeHost, RuntimeServer,
            SystemClock,
        };
        let root = std::env::temp_dir().join(format!("stable-writer-{}", Uuid::new_v4()));
        let host_root = root.join("host");
        let (mut backend, session) = super::super::tests::surface_test_backend(&root);
        let binary = root.join("controlled-provider");
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut settings = backend.settings.get();
        settings.agent_tools_enabled = false;
        settings.agent_settings_enabled = false;
        settings.subagents_enabled = false;
        settings
            .provider_binary_overrides
            .insert(ProviderKind::Codex, binary.display().to_string());
        backend.settings.replace(settings).unwrap();
        backend.stable_test_root = Some(host_root.clone());
        let factory = FakeFactory::new();
        let mut registry = AdapterRegistry::new();
        registry.register(
            "codex",
            Arc::new(factory.clone()) as Arc<dyn AdapterFactory>,
        );
        let host = RuntimeHost::open(
            &host_root,
            HostOptions::new("goddard"),
            Arc::new(registry),
            Box::new(SystemClock),
        )
        .unwrap();
        let server = RuntimeServer::bind(host, &host_root, Duration::from_millis(5)).unwrap();
        let backend = Arc::new(backend);
        backend.install_weak_self();
        let events = EventSink::detached();
        let path = root.join("repo");
        let (mut attempt, first) = backend.ensure_agent_runtime(session, &events).unwrap();
        let physical_id = backend
            .task_store
            .stable_link(session)
            .unwrap()
            .unwrap()
            .runtime_session_id;
        let witness = first.writer_completion();
        assert_eq!(witness.state(), WriterCompletionState::Pending);
        {
            drop(backend.remove_runtime(session, Some(attempt), false));
            assert!(
                backend
                    .require_no_observed_workspace_writers(&path)
                    .is_err()
            );
            let (next, attached) = backend.ensure_agent_runtime(session, &events).unwrap();
            attempt = next;
            assert!(witness.same_writer(&attached.writer_completion()));
            assert_eq!(
                backend
                    .runtime_lifecycles
                    .retired_writers
                    .lock()
                    .pending
                    .len(),
                1,
                "reattachment accumulated duplicate writer observations"
            );
        }
        drop(backend.remove_runtime(session, Some(attempt), false));
        let adapter = factory.for_session(physical_id).unwrap();
        adapter.script_terminate(waku_runtime::Termination::Unconfirmed {
            detail: "still draining".into(),
        });
        first.delete_provider_session();
        assert_eq!(witness.state(), WriterCompletionState::Pending);
        assert!(
            backend
                .require_no_observed_workspace_writers(&path)
                .is_err()
        );
        // A late positive owned exit is reconciled by the same explicit stop.
        adapter.emit(waku_runtime::ProviderEvent::Terminated {
            reason: waku_runtime::TerminateReason::Exited {
                detail: "controlled exit".into(),
            },
        });
        first.delete_provider_session();
        assert_eq!(witness.state(), WriterCompletionState::Confirmed);
        assert!(backend.require_no_observed_workspace_writers(&path).is_ok());
        let (replacement_attempt, replacement) =
            backend.ensure_agent_runtime(session, &events).unwrap();
        let replacement_witness = replacement.writer_completion();
        assert!(!witness.same_writer(&replacement_witness));
        first.delete_provider_session();
        assert_eq!(
            replacement_witness.state(),
            WriterCompletionState::Pending,
            "old stop released a replacement provider"
        );
        assert!(
            backend
                .require_no_observed_workspace_writers(&path)
                .is_err()
        );
        replacement.delete_provider_session();
        drop(backend.remove_runtime(session, Some(replacement_attempt), false));
        assert_eq!(
            replacement_witness.state(),
            WriterCompletionState::Confirmed
        );
        assert!(backend.require_no_observed_workspace_writers(&path).is_ok());
        let (natural_attempt, natural) = backend.ensure_agent_runtime(session, &events).unwrap();
        let natural_witness = natural.writer_completion();
        let natural_id = backend
            .task_store
            .stable_link(session)
            .unwrap()
            .unwrap()
            .runtime_session_id;
        factory
            .for_session(natural_id)
            .unwrap()
            .emit(waku_runtime::ProviderEvent::Terminated {
                reason: waku_runtime::TerminateReason::Exited {
                    detail: "natural exit".into(),
                },
            });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while natural_witness.state() != WriterCompletionState::Confirmed {
            assert!(
                std::time::Instant::now() < deadline,
                "natural exit did not confirm its writer"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(backend.remove_runtime(session, Some(natural_attempt), false));
        while let Err(error) = backend.require_no_observed_workspace_writers(&path) {
            assert!(
                std::time::Instant::now() < deadline,
                "natural exit cleanup precondition did not settle: {error:#}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(natural);
        drop(first);
        drop(replacement);
        drop(backend);
        drop(server);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unqualified_exit_event_cannot_authorize_workspace_removal() {
        let root = std::env::temp_dir().join(format!("workspace-unconfirmed-{}", Uuid::new_v4()));
        let (backend, _) = super::super::tests::surface_test_backend(&root);
        let path = root.join("repo");
        let (events, received) = driver::test_event_channel();
        let starting = backend
            .runtime_lifecycles
            .observe_starting_writer(&path, events.writer_completion());
        events.send(DriverEvent::ProcessExited).unwrap();
        assert!(matches!(
            received.recv().unwrap(),
            DriverEvent::ProcessExited
        ));
        drop(starting);
        drop(events);
        assert!(
            backend
                .require_no_observed_workspace_writers(&path)
                .is_err()
        );
        drop(backend);
        std::fs::remove_dir_all(root).unwrap();
    }
}
