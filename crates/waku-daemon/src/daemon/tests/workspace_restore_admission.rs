use super::*;
use std::{
    fs,
    process::{Child, Command as ProcessCommand},
    time::{Duration, Instant},
};

struct OwnedFixture(Child);
impl Drop for OwnedFixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct ReleaseFilter(PathBuf);
impl Drop for ReleaseFilter {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, b"release");
    }
}

fn git(cwd: &Path, arguments: &[&str]) {
    let output = ProcessCommand::new("git")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("-C")
        .arg(cwd)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn request(
    backend: &WakuBackend,
    operation: WorkspaceOperation,
) -> anyhow::Result<ResponsePayload> {
    backend.handle(
        Request {
            request_id: Uuid::new_v4(),
            session_id: Uuid::nil(),
            runtime_id: Uuid::nil(),
            command: Command::Workspace { operation },
        },
        EventSink::detached(),
        None,
    )
}

#[test]
fn restore_requests_exclude_retirement_before_and_through_filesystem_effects() {
    const FIXTURE_ROOT: &str = "GODDARD_RESTORE_ADMISSION_FIXTURE_ROOT";
    let Some(root) = std::env::var_os(FIXTURE_ROOT) else {
        // Isolate the process-global projectless root from other native tests.
        let root =
            std::env::temp_dir().join(format!("workspace-restore-admission-{}", Uuid::new_v4()));
        let mut child = OwnedFixture(ProcessCommand::new(std::env::current_exe().unwrap())
            .args(["--exact", "daemon::workspace_admission::tests::restore_requests_exclude_retirement_before_and_through_filesystem_effects", "--nocapture"])
            .env(FIXTURE_ROOT, &root).spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "restore fixture did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(child);
        fs::remove_dir_all(root).unwrap();
        return;
    };
    let root = PathBuf::from(root);
    let (backend, session_id) = super::super::tests::surface_test_backend(&root);
    let repo = root.join("repo");
    git(&repo, &["init", "-b", "main"]);
    let empty_hooks = root.join("empty-hooks");
    fs::create_dir_all(&empty_hooks).unwrap();
    git(
        &repo,
        &["config", "core.hooksPath", empty_hooks.to_str().unwrap()],
    );
    git(&repo, &["config", "commit.gpgSign", "false"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(repo.join("tracked.hold"), b"saved\n").unwrap();
    fs::write(repo.join(".gitattributes"), b"tracked.hold filter=pause\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "fixture"]);
    let archive_ref = crate::checkpoint::archive_ref(session_id);
    crate::checkpoint::capture_ref(&repo, &archive_ref).unwrap();
    fs::write(repo.join("tracked.hold"), b"keep-before-restore\n").unwrap();
    let peer = Arc::new(
        WakuBackend::new(
            crate::DaemonSettingsStore::open(root.join("peer-settings.json")).unwrap(),
            StateStore::daemon(backend.task_store.path().to_owned()),
        )
        .unwrap(),
    );

    let retirement = backend.begin_workspace_retirement(&repo).unwrap();
    let error = request(
        &peer,
        WorkspaceOperation::RestoreRef {
            cwd: repo.clone(),
            git_ref: archive_ref.clone(),
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("workspace admission deferred"),
        "{error:#}"
    );
    assert_eq!(
        fs::read(repo.join("tracked.hold")).unwrap(),
        b"keep-before-restore\n"
    );
    let target = root.join("restored-checkout");
    let error = request(
        &peer,
        WorkspaceOperation::EnsureWorktree {
            project_path: repo.clone(),
            path: target.clone(),
            branch: None,
            base_ref: Some(archive_ref.clone()),
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("workspace admission deferred"),
        "{error:#}"
    );
    assert!(!target.exists());
    drop(retirement);
    assert!(matches!(
        request(
            &peer,
            WorkspaceOperation::EnsureWorktree {
                project_path: repo.clone(),
                path: target.clone(),
                branch: None,
                base_ref: Some(archive_ref.clone()),
            }
        )
        .unwrap(),
        ResponsePayload::Workspace {
            result: WorkspaceResult::WorktreeEnsured { .. }
        }
    ));
    assert_eq!(fs::read(target.join("tracked.hold")).unwrap(), b"saved\n");

    // Pause inside a real Git smudge filter, after admission and before restore
    // finishes. Retirement must remain excluded for the complete operation.
    let ready = root.join("filter-ready");
    let release = ReleaseFilter(root.join("filter-release"));
    let script = root.join("restore-filter.sh");
    fs::write(&script, b"printf ready > \"$1\"\ni=0\nwhile [ ! -e \"$2\" ] && [ \"$i\" -lt 1000 ]; do sleep .01; i=$((i+1)); done\ncat\n").unwrap();
    let quote = |path: &Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"));
    git(
        &repo,
        &[
            "config",
            "filter.pause.smudge",
            &format!(
                "/bin/sh {} {} {}",
                quote(&script),
                quote(&ready),
                quote(&release.0)
            ),
        ],
    );
    let worker_peer = Arc::clone(&peer);
    let worker_repo = repo.clone();
    let worker = std::thread::spawn(move || {
        request(
            &worker_peer,
            WorkspaceOperation::RestoreRef {
                cwd: worker_repo,
                git_ref: archive_ref,
            },
        )
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(
            !worker.is_finished(),
            "restore finished before reaching the controlled filter"
        );
        assert!(
            Instant::now() < deadline,
            "restore did not reach the controlled filter"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(backend.begin_workspace_retirement(&repo).is_err());
    drop(release);
    assert!(matches!(
        worker.join().unwrap().unwrap(),
        ResponsePayload::Workspace {
            result: WorkspaceResult::Ack
        }
    ));
    assert_eq!(fs::read(repo.join("tracked.hold")).unwrap(), b"saved\n");
    drop(backend.begin_workspace_retirement(&repo).unwrap());

    let projects = root.join("projects");
    crate::projectless::set_workspace_root(Some(projects.clone()));
    let workspace = crate::projectless::create_workspace(Some("restore fixture"))
        .unwrap()
        .cwd;
    fs::write(workspace.join("notes"), b"preserve projectless data").unwrap();
    let archive = crate::projectless::archive_workspace(&workspace).unwrap();
    let retirement = backend.begin_workspace_retirement(&projects).unwrap();
    let error = request(
        &peer,
        WorkspaceOperation::RestoreProjectlessWorkspace {
            path: workspace.clone(),
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("workspace admission deferred"),
        "{error:#}"
    );
    assert!(!workspace.exists());
    assert!(archive.exists());
    drop(retirement);
    assert!(matches!(
        request(
            &peer,
            WorkspaceOperation::RestoreProjectlessWorkspace {
                path: workspace.clone()
            }
        )
        .unwrap(),
        ResponsePayload::Workspace {
            result: WorkspaceResult::Bool { value: true }
        }
    ));
    assert_eq!(
        fs::read(workspace.join("notes")).unwrap(),
        b"preserve projectless data"
    );
}

#[test]
fn projectless_creation_defers_busy_namespaces_and_skips_busy_task_names() {
    const FIXTURE_ROOT: &str = "GODDARD_PROJECTLESS_ADMISSION_FIXTURE_ROOT";
    let Some(root) = std::env::var_os(FIXTURE_ROOT) else {
        // Do not change the projectless root in the parent native test process.
        let root = std::env::temp_dir().join(format!(
            "projectless-allocation-admission-{}",
            Uuid::new_v4()
        ));
        let mut child = OwnedFixture(ProcessCommand::new(std::env::current_exe().unwrap())
            .args(["--exact", "daemon::workspace_admission::tests::projectless_creation_defers_busy_namespaces_and_skips_busy_task_names", "--nocapture"])
            .env(FIXTURE_ROOT, &root).spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "projectless fixture did not finish"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(child);
        fs::remove_dir_all(root).unwrap();
        return;
    };
    let root = PathBuf::from(root);
    let (backend, _) = super::super::tests::surface_test_backend(&root);
    let profile = root.join("fresh-profile");
    fs::create_dir_all(&profile).unwrap();
    let projects = profile.join("projects");
    crate::projectless::set_workspace_root(Some(projects.clone()));
    let create = |prompt: &str| {
        request(
            &backend,
            WorkspaceOperation::CreateProjectlessWorkspace {
                prompt: Some(prompt.to_owned()),
            },
        )
        .map(|response| match response {
            ResponsePayload::Workspace {
                result: WorkspaceResult::ProjectlessWorkspace { cwd },
            } => cwd,
            response => panic!("unexpected creation response: {response:?}"),
        })
    };
    let assert_deferred = |error: anyhow::Error| {
        assert_eq!(
            error
                .downcast_ref::<std::io::Error>()
                .map(std::io::Error::kind),
            Some(std::io::ErrorKind::WouldBlock),
            "{error:#}",
        );
    };

    // The missing root must not be created below a retiring ancestor.
    let retirement = backend.begin_workspace_retirement(&profile).unwrap();
    assert_deferred(create("allocation fixture").unwrap_err());
    assert!(!projects.exists());
    drop(retirement);
    let first = create("allocation fixture").unwrap();
    fs::write(first.join("notes"), b"keep existing task").unwrap();
    let date = first.parent().unwrap();

    // With parents already present, every candidate is blocked by retirement.
    let retirement = backend.begin_workspace_retirement(&projects).unwrap();
    let entries = fs::read_dir(date).unwrap().count();
    assert_deferred(create("other allocation").unwrap_err());
    assert_eq!(fs::read_dir(date).unwrap().count(), entries);
    drop(retirement);

    // A live existing name and a reserved absent name must not block siblings.
    let retirement = backend.begin_workspace_retirement(&first).unwrap();
    let second = create("allocation fixture").unwrap();
    assert_eq!(second, date.join("allocation-fixture-2"));
    assert_eq!(
        fs::read(first.join("notes")).unwrap(),
        b"keep existing task"
    );
    drop(retirement);
    let reserved = date.join("reserved-allocation");
    let materializing =
        WorkspaceAdmission::materialize(backend.task_store.path(), &reserved).unwrap();
    assert_eq!(
        create("reserved allocation").unwrap(),
        date.join("reserved-allocation-2")
    );
    assert!(!reserved.exists());
    drop(materializing);
    assert_eq!(create("reserved allocation").unwrap(), reserved);
}

#[test]
fn fresh_worktree_and_qa_requests_retain_source_and_allocation_admission() {
    let root =
        std::env::temp_dir().join(format!("worktree-allocation-admission-{}", Uuid::new_v4()));
    let (backend, _) = super::super::tests::surface_test_backend(&root);
    let repo = root.join("repo");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.invalid"]);
    git(&repo, &["config", "commit.gpgSign", "false"]);
    let empty_hooks = root.join("empty-hooks");
    fs::create_dir_all(&empty_hooks).unwrap();
    git(
        &repo,
        &["config", "core.hooksPath", empty_hooks.to_str().unwrap()],
    );
    fs::write(repo.join("tracked.hold"), b"saved\n").unwrap();
    fs::write(repo.join(".gitattributes"), b"tracked.hold filter=pause\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "fixture"]);
    let namespace = root.join("worktrees/repo");
    fs::create_dir_all(&namespace).unwrap();
    let peer = Arc::new(
        WakuBackend::new(
            crate::DaemonSettingsStore::open(root.join("peer-settings.json")).unwrap(),
            StateStore::daemon(backend.task_store.path().to_owned()),
        )
        .unwrap(),
    );
    for (name, carry) in [("fresh", false), ("carried", true)] {
        if carry {
            fs::write(repo.join("tracked.hold"), b"carried\n").unwrap();
        }
        let operation = || {
            if carry {
                WorkspaceOperation::CreateWorktreeFromCheckout {
                    project_path: repo.clone(),
                    name: Some(name.into()),
                }
            } else {
                WorkspaceOperation::CreateWorktree {
                    project_path: repo.clone(),
                    name: Some(name.into()),
                    base_ref: Some("HEAD".into()),
                    sync_default_branch: false,
                    sync_branches: Vec::new(),
                }
            }
        };
        let held = backend.begin_workspace_retirement(&namespace).unwrap();
        let error = request(&peer, operation()).unwrap_err();
        assert!(
            error.to_string().contains("workspace admission deferred"),
            "{error:#}"
        );
        assert!(
            !namespace.join(name).exists(),
            "refused creation allocated its name"
        );
        if carry {
            assert_eq!(fs::read(repo.join("tracked.hold")).unwrap(), b"carried\n");
        }
        drop(held);
        let ResponsePayload::Workspace {
            result: WorkspaceResult::WorktreeCreated { worktree },
        } = request(&peer, operation()).unwrap()
        else {
            panic!("creation returned the wrong result");
        };
        let expected: &[u8] = if carry { b"carried\n" } else { b"saved\n" };
        assert_eq!(
            fs::read(worktree.path.join("tracked.hold")).unwrap(),
            expected
        );
        if carry {
            assert_eq!(fs::read(repo.join("tracked.hold")).unwrap(), b"carried\n");
        }
    }

    let existing = namespace.join("fresh/repo");
    let held = backend.begin_workspace_retirement(&existing).unwrap();
    let unrelated = request(
        &peer,
        WorkspaceOperation::CreateWorktree {
            project_path: repo.clone(),
            name: Some("unrelated".into()),
            base_ref: Some("HEAD".into()),
            sync_default_branch: false,
            sync_branches: Vec::new(),
        },
    )
    .expect("retiring one checkout unnecessarily blocked an unrelated name");
    assert!(matches!(
        unrelated,
        ResponsePayload::Workspace {
            result: WorkspaceResult::WorktreeCreated { .. }
        }
    ));
    drop(held);

    // QA allocates another managed checkout through a separate RPC path.
    git(
        &repo,
        &[
            "restore",
            "--source",
            "HEAD",
            "--worktree",
            "--",
            "tracked.hold",
        ],
    );
    git(&root, &["init", "--bare", "-b", "main", "origin.git"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            root.join("origin.git").to_str().unwrap(),
        ],
    );
    git(&repo, &["push", "-u", "origin", "main"]);
    git(&repo, &["checkout", "-b", "dev"]);
    fs::write(repo.join("qa.txt"), b"review this commit\n").unwrap();
    git(&repo, &["add", "qa.txt"]);
    git(&repo, &["commit", "-m", "QA candidate"]);
    git(&repo, &["checkout", "main"]);
    let held = backend.begin_workspace_retirement(&namespace).unwrap();
    let error = request(
        &peer,
        WorkspaceOperation::QaStart {
            cwd: repo.clone(),
            branch: "dev".into(),
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("workspace admission deferred"),
        "{error:#}"
    );
    drop(held);
    assert!(matches!(
        request(
            &peer,
            WorkspaceOperation::QaStart {
                cwd: repo.clone(),
                branch: "dev".into()
            }
        )
        .unwrap(),
        ResponsePayload::Workspace {
            result: WorkspaceResult::QaStarted { .. }
        }
    ));

    // Hold a real checkout inside its smudge filter. Both its source and name
    // namespace must stay fenced until materialization has fully returned.
    let ready = root.join("allocation-ready");
    let release = ReleaseFilter(root.join("allocation-release"));
    let script = root.join("allocation-filter.sh");
    fs::write(&script, b"printf ready > \"$1\"\ni=0\nwhile [ ! -e \"$2\" ] && [ \"$i\" -lt 1000 ]; do sleep .01; i=$((i+1)); done\ncat\n").unwrap();
    let quote = |path: &Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"));
    git(
        &repo,
        &[
            "config",
            "filter.pause.smudge",
            &format!(
                "/bin/sh {} {} {}",
                quote(&script),
                quote(&ready),
                quote(&release.0)
            ),
        ],
    );
    let worker_peer = Arc::clone(&peer);
    let worker_repo = repo.clone();
    let worker = std::thread::spawn(move || {
        request(
            &worker_peer,
            WorkspaceOperation::CreateWorktree {
                project_path: worker_repo,
                name: Some("paused".into()),
                base_ref: Some("HEAD".into()),
                sync_default_branch: false,
                sync_branches: Vec::new(),
            },
        )
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(
            !worker.is_finished(),
            "allocation finished before reaching the controlled filter"
        );
        assert!(
            Instant::now() < deadline,
            "allocation did not reach the controlled filter"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    for path in [&repo, &namespace] {
        let error = backend
            .begin_workspace_retirement(path)
            .err()
            .expect("creation released its admission guard early");
        assert!(
            error.to_string().contains("workspace admission deferred"),
            "{error:#}"
        );
    }
    let paused_path = namespace.join("paused/repo");
    let error = request(
        &peer,
        WorkspaceOperation::EnsureWorktree {
            project_path: repo.clone(),
            path: paused_path.clone(),
            branch: None,
            base_ref: Some("HEAD".into()),
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("workspace admission deferred"),
        "{error:#}"
    );
    assert!(
        peer.record_native_writer_before_start(Uuid::new_v4(), Uuid::new_v4(), &paused_path)
            .is_err()
    );
    assert!(peer.task_store.native_writer_origins().unwrap().is_empty());
    assert!(
        peer.open_terminal(&paused_path, 80, 24, EventSink::detached(), None)
            .is_err()
    );
    assert!(
        peer.task_store
            .terminal_writer_origins()
            .unwrap()
            .is_empty()
    );
    drop(release);
    let ResponsePayload::Workspace {
        result: WorkspaceResult::WorktreeCreated { worktree },
    } = worker.join().unwrap().unwrap()
    else {
        panic!("creation returned the wrong result");
    };
    assert_eq!(
        fs::read(worktree.path.join("tracked.hold")).unwrap(),
        b"saved\n"
    );
    drop(backend.begin_workspace_retirement(&namespace).unwrap());
    drop(peer);
    drop(backend);
    fs::remove_dir_all(root).unwrap();
}
