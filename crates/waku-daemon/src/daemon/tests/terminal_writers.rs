#![cfg(unix)]
use super::*;

struct ReleaseBackgroundWriter {
    stop: PathBuf,
    ready: PathBuf,
    done: PathBuf,
}

impl Drop for ReleaseBackgroundWriter {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.stop, "stop");
        if self.ready.exists() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !self.done.exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

#[test]
fn terminal_writer_hold_survives_leader_close_and_daemon_restart() {
    let root = std::env::temp_dir().join(format!("terminal-writer-hold-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let cwd = backend.task_state.lock().projects[0].path.clone();
    let unrelated = root.join("unrelated");
    std::fs::create_dir_all(&unrelated).unwrap();
    let stop = root.join("stop");
    let heartbeat = cwd.join("heartbeat");
    let done = root.join("done");
    let ready = root.join("ready");
    let release = ReleaseBackgroundWriter {
        stop: stop.clone(),
        ready: ready.clone(),
        done: done.clone(),
    };
    // The background writer ignores HUP and closes its PTY descriptors. It
    // self-expires as a fallback and has an owned release marker for cleanup.
    let script = r#"
trap '' HUP
(
    trap '' HUP
    i=0
    while [ "$i" -lt 600 ] && [ ! -e "$1" ]; do
        printf . >> "$2"
        i=$((i+1))
        /bin/sleep .01
    done
    printf done > "$3"
) </dev/null >/dev/null 2>&1 &
printf ready > "$4"
while IFS= read -r line; do :; done
"#;
    let backend = backend.with_terminal_shell(alacritty_terminal::tty::Shell::new(
        "/bin/sh".into(),
        vec![
            "-c".into(),
            script.into(),
            "terminal-writer-fixture".into(),
            stop.to_string_lossy().into_owned(),
            heartbeat.to_string_lossy().into_owned(),
            done.to_string_lossy().into_owned(),
            ready.to_string_lossy().into_owned(),
        ],
    ));
    let terminal = backend
        .open_terminal(&cwd, 80, 24, EventSink::detached(), None)
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ready.exists() || !heartbeat.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "terminal writer did not start"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        backend.terminals.lock().is_empty(),
        "observation predates registry publication"
    );
    assert_eq!(
        backend.task_store.terminal_writer_origins().unwrap().len(),
        1
    );
    assert!(backend.require_no_observed_workspace_writers(&cwd).is_err());
    drop(terminal);
    let before = std::fs::metadata(&heartbeat).unwrap().len();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::fs::metadata(&heartbeat).unwrap().len() <= before {
        assert!(
            std::time::Instant::now() < deadline,
            "background writer did not outlive terminal close"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(backend.require_no_observed_workspace_writers(&cwd).is_err());
    drop(backend);
    let reopened = reopen_backend(&root);
    assert!(
        reopened
            .require_no_observed_workspace_writers(&cwd)
            .is_err()
    );
    assert!(
        reopened
            .require_no_observed_workspace_writers(&unrelated)
            .is_ok()
    );
    drop(release);
    assert!(done.exists(), "owned background writer did not finish");
    // A reused path cannot masquerade as the directory observed at spawn.
    let moved = root.join("moved-workspace");
    std::fs::rename(&cwd, &moved).unwrap();
    std::fs::create_dir(&cwd).unwrap();
    assert!(
        reopened
            .require_no_observed_workspace_writers(&cwd)
            .is_err()
    );
    // Restore the owned fixture identity before testing the unrelated path.
    std::fs::remove_dir(&cwd).unwrap();
    std::fs::rename(&moved, &cwd).unwrap();
    assert!(
        reopened
            .require_no_observed_workspace_writers(&unrelated)
            .is_ok()
    );
    assert!(
        reopened
            .require_no_observed_workspace_writers(&cwd)
            .is_err()
    );
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn terminal_writer_record_failure_prevents_the_shell_from_spawning() {
    let root =
        std::env::temp_dir().join(format!("terminal-writer-failed-record-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let cwd = backend.task_state.lock().projects[0].path.clone();
    let marker = root.join("unexpected-child");
    let backend = backend.with_terminal_shell(alacritty_terminal::tty::Shell::new(
        "/bin/sh".into(),
        vec![
            "-c".into(),
            "printf child > \"$1\"".into(),
            "fixture".into(),
            marker.to_string_lossy().into_owned(),
        ],
    ));
    rusqlite::Connection::open(backend.task_store.path())
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_terminal_writer BEFORE INSERT ON terminal_workspace_writers
         BEGIN SELECT RAISE(ABORT, 'injected terminal writer persistence failure'); END;",
        )
        .unwrap();
    let result = backend.open_terminal(&cwd, 80, 24, EventSink::detached(), None);
    assert!(result.is_err());
    assert!(!marker.exists());
    assert!(
        backend
            .task_store
            .terminal_writer_origins()
            .unwrap()
            .is_empty()
    );
    assert!(backend.require_no_observed_workspace_writers(&cwd).is_ok());
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn terminal_validation_failure_does_not_record_an_unattempted_writer() {
    let root = std::env::temp_dir().join(format!("terminal-writer-validation-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let backend = backend.with_terminal_shell(alacritty_terminal::tty::Shell::new(
        "/bin/sh".into(),
        Vec::new(),
    ));
    assert!(
        backend
            .open_terminal(&root.join("missing"), 80, 24, EventSink::detached(), None)
            .is_err()
    );
    assert!(
        backend
            .task_store
            .terminal_writer_origins()
            .unwrap()
            .is_empty()
    );
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_worktree_removal_checks_writers_across_the_entire_git_root() {
    let root = std::env::temp_dir().join(format!("nested-worktree-writer-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let repo = backend.task_state.lock().projects[0].path.clone();
    let git = |args: &[&str]| {
        let output = crate::command_env::search_path_command("git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
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
    git(&["init", "-q", "--template="]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--allow-empty",
        "-qm",
        "fixture",
    ]);
    let worktree = root.join("linked");
    let clean = root.join("clean-linked");
    git(&["worktree", "add", "--detach", worktree.to_str().unwrap()]);
    git(&["worktree", "add", "--detach", clean.to_str().unwrap()]);
    let project = worktree.join("project");
    let sibling = worktree.join("sibling");
    let clean_project = clean.join("project");
    for path in [&project, &sibling, &clean_project] {
        std::fs::create_dir(path).unwrap();
    }
    let contents = sibling.join("preserve.txt");
    std::fs::write(&contents, "sibling work must survive").unwrap();
    let backend = backend.with_terminal_shell(alacritty_terminal::tty::Shell::new(
        "/bin/sh".into(),
        vec!["-c".into(), "exit 0".into()],
    ));
    let terminal = backend
        .open_terminal(&sibling, 80, 24, EventSink::detached(), None)
        .unwrap();
    drop(terminal);
    // The old preflight accepted this nested project even though Git's
    // actual deletion also contains the sibling's unresolved writer origin.
    assert!(
        backend
            .require_no_observed_workspace_writers(&project)
            .is_ok()
    );
    let capture = |cwd: PathBuf, git_ref: String| {
        backend.handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::Workspace {
                    operation: WorkspaceOperation::CaptureRef { cwd, git_ref },
                },
            },
            EventSink::detached(),
            None,
        )
    };
    let blocked_ref = crate::checkpoint::archive_ref(Uuid::new_v4());
    let error = capture(project.clone(), blocked_ref.clone()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("terminal descendants have no confirmed completion")
    );
    assert!(!crate::checkpoint::has_ref(&worktree, &blocked_ref));
    let remove = |path: PathBuf, force| {
        backend.handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::Workspace {
                    operation: WorkspaceOperation::RemoveWorktree { path, force },
                },
            },
            EventSink::detached(),
            None,
        )
    };
    let error = remove(project.clone(), true).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("terminal descendants have no confirmed completion")
    );
    assert!(worktree.is_dir());
    assert!(project.is_dir());
    assert_eq!(
        std::fs::read_to_string(contents).unwrap(),
        "sibling work must survive"
    );
    // An unrelated archive captures the sibling files outside the project,
    // then its whole worktree still removes through the nested project path.
    let clean_sibling = clean.join("sibling.txt");
    std::fs::write(&clean_sibling, "archive this sibling").unwrap();
    let preserved_ref = crate::checkpoint::archive_ref(Uuid::new_v4());
    capture(clean_project.clone(), preserved_ref.clone()).unwrap();
    let archived = crate::command_env::search_path_command("git")
        .args(["show", &format!("{preserved_ref}:sibling.txt")])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(archived.status.success());
    assert_eq!(archived.stdout, b"archive this sibling");
    assert!(matches!(
        remove(clean_project, true).unwrap(),
        ResponsePayload::Workspace {
            result: WorkspaceResult::Ack
        }
    ));
    assert!(!clean.exists());
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn retirement_excludes_local_writer_admissions_and_releases_after_failure() {
    let root = std::env::temp_dir().join(format!("workspace-admission-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let workspace = root.join("repo");
    let nested = workspace.join("nested");
    let unrelated = root.join("unrelated");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(&unrelated).unwrap();
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&nested, &alias).unwrap();
    let marker = root.join("spawned");
    let backend = backend.with_terminal_shell(alacritty_terminal::tty::Shell::new(
        "/bin/sh".into(),
        vec![
            "-c".into(),
            "printf child > \"$1\"".into(),
            "fixture".into(),
            marker.to_string_lossy().into_owned(),
        ],
    ));
    let retirement = backend.begin_workspace_retirement(&workspace).unwrap();
    // An alias and an ancestor both intersect the reservation; siblings do not.
    for path in [&alias, &root] {
        let error = backend
            .runtime_lifecycles
            .observe_starting_writer(path, driver::WriterCompletion::default())
            .err()
            .expect("intersecting provider admission was allowed");
        assert!(error.to_string().contains("workspace admission deferred"));
        assert!(backend.begin_workspace_retirement(path).is_err());
    }
    // Retirement work serializes, but an unrelated writer can still publish.
    assert!(backend.begin_workspace_retirement(&unrelated).is_err());
    drop(backend.admit_workspace_writer(&unrelated).unwrap());
    let other = WakuBackend::new(
        crate::DaemonSettingsStore::open(root.join("other-settings.json")).unwrap(),
        StateStore::daemon(backend.task_store.path().to_owned()),
    )
    .unwrap();
    // Its independent in-memory registry cannot see our local retirement.
    // Each physical-contact boundary must consult the shared store's gate.
    for path in [&alias, &root] {
        assert!(
            other
                .record_native_writer_before_start(Uuid::new_v4(), Uuid::new_v4(), path)
                .is_err()
        );
        assert!(
            other
                .record_checkpoint_writer_before_capture(path, Uuid::new_v4(), 1)
                .is_err()
        );
        assert!(other.admit_workspace_writer(path).is_err());
        assert!(
            other
                .open_terminal(path, 80, 24, EventSink::detached(), None)
                .is_err()
        );
    }
    assert!(other.task_store.native_writer_origins().unwrap().is_empty());
    assert!(
        other
            .task_store
            .checkpoint_writer_origins()
            .unwrap()
            .is_empty()
    );
    assert!(
        other
            .task_store
            .terminal_writer_origins()
            .unwrap()
            .is_empty()
    );
    drop(other);
    let error = backend
        .open_terminal(&alias, 80, 24, EventSink::detached(), None)
        .err()
        .expect("terminal admission was allowed during retirement");
    assert!(error.to_string().contains("workspace admission deferred"));
    assert!(!marker.exists());
    assert!(
        backend
            .task_store
            .terminal_writer_origins()
            .unwrap()
            .is_empty()
    );
    drop(retirement);
    let retry = backend.begin_workspace_retirement(&workspace).unwrap();
    drop(retry);
    let terminal = backend
        .open_terminal(&alias, 80, 24, EventSink::detached(), None)
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !marker.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "admitted terminal did not run"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    drop(terminal);
    // Quiet-check failure must release its reservation, without clearing the
    // durable unqualified terminal origin or blocking unrelated operations.
    assert!(backend.begin_workspace_retirement(&workspace).is_err());
    let witness = driver::WriterCompletion::default();
    witness.register();
    let admitted = backend
        .runtime_lifecycles
        .observe_starting_writer(&workspace, witness.clone())
        .expect("failed quiet check retained its admission reservation");
    witness.confirm();
    drop(admitted);
    drop(backend.begin_workspace_retirement(&unrelated).unwrap());
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}
