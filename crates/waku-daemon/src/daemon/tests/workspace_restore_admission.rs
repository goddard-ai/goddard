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
