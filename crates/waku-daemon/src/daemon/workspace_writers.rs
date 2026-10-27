//! Observe provider and checkpoint writers without retaining driver handles.
//! This is a necessary cleanup precondition, not a filesystem-removal permit.
//! Archive authority, shared owners, other writers, and admission still need
//! their own checks at the final destructive boundary.

use super::*;
use driver::{WriterCompletion, WriterCompletionState};

/// Preserve an unqualified terminal origin before its PTY can spawn. Neither
/// close, EOF nor a shell-leader wait can qualify arbitrary descendants.
pub(super) struct TerminalWriterEvents {
    pub events: EventSink,
    pub store: Arc<StateStore>,
    pub lifecycles: Arc<RuntimeLifecycles>,
}

pub(super) fn terminal_directory_identity(path: &Path) -> anyhow::Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path)?;
        anyhow::ensure!(metadata.is_dir(), "terminal origin is not a directory");
        Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        anyhow::bail!("terminal writer identity is unavailable on this platform")
    }
}

impl crate::terminal::TerminalEventSink for TerminalWriterEvents {
    fn before_spawn(&self, cwd: &Path) -> anyhow::Result<()> {
        let workspace = dunce::canonicalize(cwd)?;
        let identity = terminal_directory_identity(&workspace)?;
        // Commit the durable origin while publication is excluded by the
        // same short gate that admits destructive workspace operations.
        let writers = self.lifecycles.retired_writers.lock();
        writers.require_admission(&workspace)?;
        self.store
            .record_terminal_writer(&crate::persistence::TerminalWriterOrigin {
                workspace,
                identity,
            })?;
        drop(writers);
        Ok(())
    }

    fn send_ephemeral(&self, event: WireDriverEvent) -> anyhow::Result<()> {
        self.events.send_ephemeral(event)
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct CheckpointCaptureKey {
    cwd: PathBuf,
    session_id: Uuid,
    turn_count: usize,
}

#[derive(Default)]
pub(super) struct RetiredWriters {
    retiring: HashMap<Uuid, PathBuf>,
    pending: Vec<(PathBuf, WriterCompletion)>,
    // An unqualified transport leaves one conservative marker per workspace,
    // rather than retaining a driver or accumulating one marker per restart.
    unknown: HashSet<PathBuf>,
    // A capture/persistence request is still owed even when its own physical
    // Git work is known quiet. Matching successful retries can fulfill it;
    // they must never erase the unrelated unknown-provider set above.
    checkpoints: HashSet<CheckpointCaptureKey>,
}

impl RetiredWriters {
    fn require_admission(&self, path: &Path) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.retiring.values().any(|root| paths_overlap(path, root)),
            "workspace admission deferred: an intersecting workspace operation is in progress"
        );
        Ok(())
    }
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

/// A process-local admission fence, not durable archive authority or proof
/// about writers belonging to another daemon. No registry lock spans Git I/O.
pub(super) struct WorkspaceRetirement {
    lifecycles: Arc<RuntimeLifecycles>,
    id: Uuid,
}

impl Drop for WorkspaceRetirement {
    fn drop(&mut self) {
        self.lifecycles
            .retired_writers
            .lock()
            .retiring
            .remove(&self.id);
    }
}

/// Keep attempted launches visible before a RuntimeEntry exists. A failed
/// factory call can already have contacted a provider; absence of an entry
/// therefore cannot release its unconfirmed writer observation.
/// Checkpoint workers use the same observation before waiting for capture.
pub(super) struct StartingWriter {
    lifecycles: Arc<RuntimeLifecycles>,
    witness: WriterCompletion,
}

impl StartingWriter {
    /// Collapse an unsuccessful worker's unqualified evidence to one marker
    /// per workspace instead of accumulating a witness on every retry.
    pub(super) fn unqualified(self) {
        self.retain_unknown();
    }

    fn retain_unknown(&self) {
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
            self.retain_unknown();
        }
    }
}

impl RuntimeLifecycles {
    pub(super) fn checkpoint_owed(
        &self,
        cwd: &Path,
        session_id: Uuid,
        turn_count: usize,
    ) -> CheckpointCaptureKey {
        let key = CheckpointCaptureKey {
            cwd: dunce::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned()),
            session_id,
            turn_count,
        };
        // Publish debt before entering the capture lock. A successful same-key
        // capture can fulfill concurrent duplicate requests too; late failures
        // never reinsert already-fulfilled debt.
        self.retired_writers.lock().checkpoints.insert(key.clone());
        key
    }

    pub(super) fn checkpoint_completed(
        &self,
        key: &CheckpointCaptureKey,
        witness: &WriterCompletion,
        persisted: bool,
    ) {
        let mut writers = self.retired_writers.lock();
        if persisted {
            writers.checkpoints.remove(key);
        }
        // Publish debt/fulfillment and physical completion under the same lock
        // observed by cleanup; there is no unaccounted writer transition.
        witness.confirm();
    }

    pub(super) fn observe_starting_writer(
        self: &Arc<Self>,
        cwd: &Path,
        witness: WriterCompletion,
    ) -> anyhow::Result<StartingWriter> {
        let cwd = dunce::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
        let mut writers = self.retired_writers.lock();
        writers.require_admission(&cwd)?;
        writers
            .pending
            .retain(|(_, witness)| witness.state() != WriterCompletionState::Confirmed);
        writers.pending.push((cwd, witness.clone()));
        Ok(StartingWriter {
            lifecycles: Arc::clone(self),
            witness,
        })
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
            .chain(retired.checkpoints.iter().map(|key| key.cwd.clone()))
            .collect()
    }
}

impl WakuBackend {
    pub(super) fn record_native_writer_before_start(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        cwd: &Path,
    ) -> anyhow::Result<Uuid> {
        // Do not keep admitting attempts while known completion receipts
        // cannot be persisted. Retry without waiting for idle maintenance.
        self.retry_native_writer_confirmations()?;
        let workspace = dunce::canonicalize(cwd)?;
        #[cfg(unix)]
        let identity = Some(terminal_directory_identity(&workspace)?);
        #[cfg(not(unix))]
        let identity = None;
        let origin_id = Uuid::new_v4();
        self.task_store
            .record_native_writer(&crate::persistence::NativeWriterOrigin {
                origin_id,
                session_id,
                runtime_id,
                workspace,
                identity,
            })?;
        Ok(origin_id)
    }

    pub(super) fn observe_native_writer_completion(
        &self,
        origin_id: Uuid,
        witness: WriterCompletion,
    ) {
        let store = Arc::clone(&self.task_store);
        let lifecycles = Arc::downgrade(&self.runtime_lifecycles);
        witness.on_confirm(move || {
            let lifecycles = lifecycles.upgrade();
            if let Some(lifecycles) = &lifecycles {
                lifecycles.native_confirmations.lock().insert(origin_id);
            }
            match store.confirm_native_writer(origin_id) {
                Ok(()) => {
                    if let Some(lifecycles) = lifecycles {
                        lifecycles.native_confirmations.lock().remove(&origin_id);
                    }
                }
                Err(error) => {
                    eprintln!("could not persist native writer completion {origin_id}: {error}")
                }
            }
        });
    }

    fn retry_native_writer_confirmations(&self) -> anyhow::Result<()> {
        let pending: Vec<_> = self
            .runtime_lifecycles
            .native_confirmations
            .lock()
            .iter()
            .take(64)
            .copied()
            .collect();
        let mut failure = None;
        for origin_id in pending {
            match self.task_store.confirm_native_writer(origin_id) {
                Ok(()) => {
                    self.runtime_lifecycles
                        .native_confirmations
                        .lock()
                        .remove(&origin_id);
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        if let Some(error) = failure {
            return Err(error.into());
        }
        Ok(())
    }

    /// Exclude intersecting local writer admissions before checking quiet, and
    /// retain the reservation through the caller's complete filesystem effect.
    /// Cross-daemon admission and archive-generation checks remain separate.
    pub(super) fn begin_workspace_retirement(
        &self,
        path: &Path,
    ) -> anyhow::Result<WorkspaceRetirement> {
        let target = dunce::canonicalize(path)?;
        let id = Uuid::new_v4();
        {
            let mut writers = self.runtime_lifecycles.retired_writers.lock();
            writers.require_admission(&target)?;
            writers.retiring.insert(id, target.clone());
        }
        let retirement = WorkspaceRetirement {
            lifecycles: Arc::clone(&self.runtime_lifecycles),
            id,
        };
        self.require_no_observed_workspace_writers(&target)?;
        Ok(retirement)
    }

    /// Worker-only: normalize aliases outside the runtime locks, then reject
    /// removal while an observed provider is live or its stop is unconfirmed.
    /// Successful return does not authorize deletion or exclude a new launch;
    /// the retirement executor must also hold its workspace admission fence.
    pub(super) fn require_no_observed_workspace_writers(&self, path: &Path) -> anyhow::Result<()> {
        self.retry_native_writer_confirmations()?;
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
                "workspace cleanup deferred: a workspace writer is still active or its stop is unconfirmed"
            );
        }
        let native_origins = self.task_store.native_writer_origins()?;
        anyhow::ensure!(
            native_origins.len() < 65,
            "workspace cleanup deferred: native writer inventory exceeds the bounded safety scan"
        );
        for origin in native_origins {
            let canonical = dunce::canonicalize(&origin.workspace)
                .context("workspace cleanup deferred: native writer origin is unavailable")?;
            #[cfg(unix)]
            let current_identity = Some(terminal_directory_identity(&canonical)?);
            #[cfg(not(unix))]
            let current_identity: Option<String> = None;
            anyhow::ensure!(
                canonical == origin.workspace
                    && origin.identity.is_some()
                    && origin.identity == current_identity,
                "workspace cleanup deferred: native writer origin moved, was reused or is unqualified"
            );
            anyhow::ensure!(
                !(canonical.starts_with(&target) || target.starts_with(&canonical)),
                "workspace cleanup deferred: a durable native writer has no confirmed completion"
            );
        }
        self.require_no_observed_checkpoint_writers(&target)?;
        let terminal_origins = self.task_store.terminal_writer_origins()?;
        anyhow::ensure!(
            terminal_origins.len() < 65,
            "workspace cleanup deferred: terminal writer inventory exceeds the bounded safety scan"
        );
        for origin in terminal_origins {
            // A moved, reused or unreadable origin cannot be located safely
            // from its old pathname; preserve data rather than skip it.
            let canonical = dunce::canonicalize(&origin.workspace)
                .context("workspace cleanup deferred: terminal writer origin is unavailable")?;
            anyhow::ensure!(
                canonical == origin.workspace
                    && terminal_directory_identity(&canonical)? == origin.identity,
                "workspace cleanup deferred: terminal writer origin moved or its path was reused"
            );
            anyhow::ensure!(
                !(canonical.starts_with(&target) || target.starts_with(&canonical)),
                "workspace cleanup deferred: terminal descendants have no confirmed completion"
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
                .observe_starting_writer(&worktree, events.writer_completion())
                .unwrap();
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
    fn ending_checkpoint_remains_a_writer_until_capture_and_persistence_finish() {
        let root = std::env::temp_dir().join(format!("checkpoint-writer-{}", Uuid::new_v4()));
        let (backend, session) = super::super::tests::surface_test_backend(&root);
        let path = root.join("repo");
        for args in [
            vec!["init", "-q"],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "seed",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .current_dir(&path)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(path.join("ending.txt"), "final changes").unwrap();
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        let count = backend
            .task_store
            .load_session_detail(session)
            .unwrap()
            .unwrap()
            .turns
            .last()
            .unwrap()
            .turn_count;
        let capture_lock = Arc::new(Mutex::new(()));
        backend
            .checkpoint_capture_locks
            .lock()
            .insert(path.clone(), capture_lock.clone());
        let held = capture_lock.lock();
        let backend = Arc::new(backend);
        let capturing = {
            let backend = backend.clone();
            let path = path.clone();
            std::thread::spawn(move || {
                backend.handle(
                    Request {
                        request_id: Uuid::new_v4(),
                        session_id: session,
                        runtime_id: Uuid::nil(),
                        command: Command::Workspace {
                            operation: waku_protocol::WorkspaceOperation::CaptureTurn {
                                cwd: path,
                                session_id: session,
                                turn_count: count,
                                untouched: false,
                            },
                        },
                    },
                    EventSink::detached(),
                    None,
                )
            })
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while backend
            .require_no_observed_workspace_writers(&alias)
            .is_ok()
        {
            assert!(
                std::time::Instant::now() < deadline,
                "queued checkpoint was invisible to cleanup"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let removal = backend.handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::Workspace {
                    operation: waku_protocol::WorkspaceOperation::RemoveWorktree {
                        path: alias.clone(),
                        force: true,
                    },
                },
            },
            EventSink::detached(),
            None,
        );
        assert!(
            removal
                .unwrap_err()
                .to_string()
                .contains("writer is still active")
        );
        assert!(path.join("ending.txt").is_file());
        drop(held);
        let result = capturing.join().unwrap().unwrap();
        assert!(
            matches!(result, ResponsePayload::Workspace { result: waku_protocol::WorkspaceResult::Checkpoint { checkpoint } } if checkpoint.status == crate::model::CheckpointStatus::Ready)
        );
        backend
            .require_no_observed_workspace_writers(&alias)
            .unwrap();
        let stored = backend
            .task_store
            .load_session_detail(session)
            .unwrap()
            .unwrap();
        assert_eq!(
            stored
                .turns
                .last()
                .unwrap()
                .checkpoint
                .as_ref()
                .unwrap()
                .status,
            crate::model::CheckpointStatus::Ready
        );
        drop(backend);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_checkpoint_persistence_keeps_retryable_debt_without_erasing_unknown_writers() {
        let root =
            std::env::temp_dir().join(format!("checkpoint-failure-writer-{}", Uuid::new_v4()));
        let (backend, session) = super::super::tests::surface_test_backend(&root);
        let path = root.join("repo");
        let count = backend
            .task_store
            .load_session_detail(session)
            .unwrap()
            .unwrap()
            .turns
            .last()
            .unwrap()
            .turn_count;
        // A queued capture cannot start Git while retirement owns admission.
        // Its obligation survives denial and a subsequent retry fulfills it.
        let retirement = backend.begin_workspace_retirement(&path).unwrap();
        let error = backend
            .capture_turn_checkpoint(path.clone(), session, count, false)
            .unwrap_err();
        assert!(error.to_string().contains("workspace admission deferred"));
        assert_eq!(
            backend
                .runtime_lifecycles
                .retired_writers
                .lock()
                .checkpoints
                .len(),
            1
        );
        drop(retirement);
        rusqlite::Connection::open(backend.task_store.path()).unwrap().execute_batch(
            "CREATE TRIGGER fail_checkpoint BEFORE UPDATE ON session_details BEGIN SELECT RAISE(ABORT, 'fixture checkpoint persistence failure'); END;"
        ).unwrap();
        for _ in 0..3 {
            assert!(
                backend
                    .capture_turn_checkpoint(path.clone(), session, count, false)
                    .is_err()
            );
            assert!(
                backend
                    .require_no_observed_workspace_writers(&path)
                    .is_err()
            );
        }
        let observations = backend.runtime_lifecycles.retired_writers.lock();
        assert!(
            observations.unknown.is_empty(),
            "completed Git work became unknown ownership"
        );
        assert_eq!(observations.checkpoints.len(), 1);
        assert!(
            observations
                .pending
                .iter()
                .all(|(_, witness)| witness.state() == WriterCompletionState::Confirmed),
            "failed persistence left an unconfirmed physical capture"
        );
        drop(observations);
        // A distinct provider whose ownership is unknown cannot be cleared by
        // a checkpoint retry, even in the same workspace.
        let unknown = backend
            .runtime_lifecycles
            .observe_starting_writer(&path, WriterCompletion::default())
            .unwrap();
        drop(unknown);
        rusqlite::Connection::open(backend.task_store.path())
            .unwrap()
            .execute_batch("DROP TRIGGER fail_checkpoint;")
            .unwrap();
        assert!(
            backend
                .capture_turn_checkpoint(path.clone(), session, count, false)
                .is_ok()
        );
        assert!(
            backend
                .runtime_lifecycles
                .retired_writers
                .lock()
                .checkpoints
                .is_empty()
        );
        assert!(
            backend
                .require_no_observed_workspace_writers(&path)
                .is_err(),
            "successful retry cleared unrelated provider uncertainty"
        );
        // Exercise the same production retry path without an unknown provider.
        let other = root.join("other");
        let (clean_backend, clean_session) = super::super::tests::surface_test_backend(&other);
        let clean_path = other.join("repo");
        let clean_count = clean_backend
            .task_store
            .load_session_detail(clean_session)
            .unwrap()
            .unwrap()
            .turns
            .last()
            .unwrap()
            .turn_count;
        let connection = rusqlite::Connection::open(clean_backend.task_store.path()).unwrap();
        connection.execute_batch("CREATE TRIGGER fail_checkpoint BEFORE UPDATE ON session_details BEGIN SELECT RAISE(ABORT, 'fixture checkpoint persistence failure'); END;").unwrap();
        assert!(
            clean_backend
                .capture_turn_checkpoint(clean_path.clone(), clean_session, clean_count, false)
                .is_err()
        );
        assert!(
            clean_backend
                .require_no_observed_workspace_writers(&clean_path)
                .is_err()
        );
        connection
            .execute_batch("DROP TRIGGER fail_checkpoint;")
            .unwrap();
        assert!(
            clean_backend
                .capture_turn_checkpoint(clean_path.clone(), clean_session, clean_count + 1, false)
                .is_ok()
        );
        assert!(
            clean_backend
                .require_no_observed_workspace_writers(&clean_path)
                .is_err(),
            "a different checkpoint cleared the failed capture's debt"
        );
        assert!(
            clean_backend
                .capture_turn_checkpoint(clean_path.clone(), clean_session, clean_count, false)
                .is_ok()
        );
        clean_backend
            .require_no_observed_workspace_writers(&clean_path)
            .unwrap();
        drop(clean_backend);
        drop(backend);
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
            .observe_starting_writer(&path, events.writer_completion())
            .unwrap();
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
