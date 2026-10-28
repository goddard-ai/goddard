use super::*;
use std::cell::Cell;
use std::io::{Read as _, Write as _};
use std::process::{Child, Command as ProcessCommand};
use std::time::{Duration, Instant};

struct OwnedFixture(Child);
impl Drop for OwnedFixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn fixture(root: &Path, name: &str) -> (WakuBackend, CleanupToken, PathBuf) {
    let source = root.join("projects/2099-01-01").join(name);
    fs::create_dir_all(source.join("nested/empty")).unwrap();
    fs::write(source.join("payload"), vec![b'x'; 256 * 1024]).unwrap();
    fs::write(source.join("nested/keep"), b"retained").unwrap();
    let store = StateStore::daemon(root.join(format!("{name}.db")));
    let mut state = PersistedState::fresh(source.clone());
    state.sessions[0].begin_turn("fixture");
    state.sessions[0].finish_active_turn(TurnStatus::Completed);
    state.sessions[0].archived_at = Some(100);
    let session_id = state.sessions[0].id;
    store.save(&mut state).unwrap();
    let generation = store
        .workspace_cleanup_generation(session_id)
        .unwrap()
        .unwrap();
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join(format!("{name}.settings.json"))).unwrap(),
        store,
    )
    .unwrap();
    (
        backend,
        CleanupToken {
            session_id,
            generation,
        },
        source,
    )
}

fn locations(backend: &WakuBackend, token: CleanupToken) -> ArchiveLocations {
    let run = backend
        .task_store
        .workspace_cleanup_run(token)
        .unwrap()
        .unwrap();
    let recovery: Recovery = serde_json::from_str(&run.payload).unwrap();
    ArchiveLocations::new(&recovery.root, &recovery.source, recovery.attempt).unwrap()
}

fn read_payload(path: &Path) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    archive
        .by_name("payload")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(archive.by_name("nested/empty/").unwrap().is_dir());
    bytes
}

#[test]
fn projectless_preservation_defers_cancels_recovers_and_preserves_authority() {
    const ENV: &str = "GODDARD_PROJECTLESS_IDLE_FIXTURE_ROOT";
    let Some(root) = std::env::var_os(ENV) else {
        // The configured root is process-global. Keep this fixture isolated
        // from other native tests and never start an application or provider.
        let root = std::env::temp_dir().join(format!("projectless-idle-{}", Uuid::new_v4()));
        let mut child = OwnedFixture(ProcessCommand::new(std::env::current_exe().unwrap())
            .args(["--exact", "daemon::projectless_cleanup::tests::projectless_preservation_defers_cancels_recovers_and_preserves_authority", "--nocapture"])
            .env(ENV, &root).spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "idle preservation fixture did not finish"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(child);
        fs::remove_dir_all(root).unwrap();
        return;
    };
    let root = PathBuf::from(root);
    let projects = root.join("projects");
    waku_base::projectless::set_workspace_root(Some(projects.clone()));

    let (backend, token, _) = fixture(&root, "retry");
    backend
        .task_store
        .retry_workspace_cleanup(token, 1000, true)
        .unwrap();
    assert!(
        backend
            .task_store
            .advance_workspace_cleanup_queue(1001, || true)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        backend
            .task_store
            .advance_workspace_cleanup_queue(1002, || true)
            .unwrap(),
        vec![token]
    );
    backend
        .task_store
        .retry_workspace_cleanup(token, 1002, true)
        .unwrap();
    assert!(
        backend
            .task_store
            .advance_workspace_cleanup_queue(1005, || true)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        backend
            .task_store
            .advance_workspace_cleanup_queue(1006, || true)
            .unwrap(),
        vec![token]
    );
    backend
        .task_store
        .retry_workspace_cleanup(token, 1006, false)
        .unwrap();
    assert!(
        backend
            .task_store
            .advance_workspace_cleanup_queue(1035, || true)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        backend
            .task_store
            .advance_workspace_cleanup_queue(1036, || true)
            .unwrap(),
        vec![token]
    );
    drop(backend);

    let (backend, token, source) = fixture(&root, "cancel");
    assert!(matches!(
        backend
            .advance_projectless_cleanup(token, &projects, || false)
            .unwrap(),
        Progress::More
    ));
    assert!(
        backend
            .task_store
            .workspace_cleanup_run(token)
            .unwrap()
            .is_none()
    );
    let paused = Cell::new(false);
    let control = || {
        if let Some(run) = backend.task_store.workspace_cleanup_run(token).unwrap() {
            let record: Recovery = serde_json::from_str(&run.payload).unwrap();
            let paths = ArchiveLocations::new(&projects, &source, record.attempt).unwrap();
            if run.phase == "capture"
                && record.partial_identity.is_some()
                && fs::metadata(&paths.partial).is_ok_and(|file| file.len() > 0)
            {
                paused.set(true);
                return false;
            }
        }
        true
    };
    assert!(matches!(
        backend
            .advance_projectless_cleanup(token, &projects, control)
            .unwrap(),
        Progress::More
    ));
    assert!(paused.get());
    let paths = locations(&backend, token);
    assert!(!paths.archive.exists());
    assert_eq!(
        fs::read(source.join("payload")).unwrap(),
        vec![b'x'; 256 * 1024]
    );
    // Pressure cancellation releases both source and destination admissions.
    drop(WorkspaceAdmission::writer(backend.task_store.path(), &source).unwrap());
    fs::write(source.join("payload"), b"updated after cancellation").unwrap();
    let db = backend.task_store.path().to_owned();
    drop(backend);
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("cancel.settings.json")).unwrap(),
        StateStore::daemon(db),
    )
    .unwrap();
    assert!(matches!(
        backend
            .advance_projectless_cleanup(token, &projects, || true)
            .unwrap(),
        Progress::Complete
    ));
    assert_eq!(read_payload(&paths.archive), b"updated after cancellation");
    assert_eq!(fs::read(source.join("nested/keep")).unwrap(), b"retained");
    assert!(!paths.partial.exists());
    let preserved = backend
        .task_store
        .workspace_cleanup_run(token)
        .unwrap()
        .unwrap();
    assert_eq!(preserved.phase, "preserved");
    assert!(
        backend
            .task_store
            .workspace_cleanup_descriptor(token)
            .unwrap()
            .is_some(),
        "preservation must not mark retirement complete"
    );
    assert!(
        backend
            .task_store
            .advance_workspace_cleanup_queue(u64::MAX, || true)
            .unwrap()
            .is_empty(),
        "preserved jobs must not resubmit heavy compression"
    );
    let identity = Identity::read(&paths.archive, false).unwrap();
    backend
        .advance_projectless_cleanup(token, &projects, || true)
        .unwrap();
    assert_eq!(
        Identity::read(&paths.archive, false).unwrap(),
        identity,
        "unchanged preservation repeated compression"
    );
    drop(backend);

    let (backend, token, source) = fixture(&root, "corrupt");
    let corrupted = Cell::new(false);
    let control = || {
        if let Some(run) = backend.task_store.workspace_cleanup_run(token).unwrap() {
            if run.phase == "verify" && !corrupted.replace(true) {
                let paths = locations(&backend, token);
                fs::OpenOptions::new()
                    .write(true)
                    .open(paths.partial)
                    .unwrap()
                    .set_len(12)
                    .unwrap();
            }
        }
        true
    };
    assert!(
        backend
            .advance_projectless_cleanup(token, &projects, control)
            .is_err()
    );
    assert!(corrupted.get());
    let paths = locations(&backend, token);
    assert!(!paths.archive.exists());
    assert_eq!(
        fs::read(source.join("payload")).unwrap(),
        vec![b'x'; 256 * 1024]
    );
    backend
        .advance_projectless_cleanup(token, &projects, || true)
        .unwrap();
    assert_eq!(read_payload(&paths.archive), vec![b'x'; 256 * 1024]);
    drop(backend);

    let (backend, token, source) = fixture(&root, "stale");
    backend
        .advance_projectless_cleanup(token, &projects, || {
            backend
                .task_store
                .workspace_cleanup_run(token)
                .unwrap()
                .is_none()
        })
        .unwrap();
    assert!(
        backend
            .task_store
            .workspace_cleanup_run(token)
            .unwrap()
            .is_some()
    );
    let peer = StateStore::daemon(backend.task_store.path().to_owned());
    let mut state = peer.load().unwrap();
    state.sessions[0].archived_at = None;
    state.mark_session_dirty(token.session_id);
    peer.save(&mut state).unwrap();
    assert!(matches!(
        backend
            .advance_projectless_cleanup(token, &projects, || true)
            .unwrap(),
        Progress::Complete
    ));
    assert_eq!(fs::read(source.join("nested/keep")).unwrap(), b"retained");
    state.sessions[0].archived_at = Some(200);
    state.mark_session_dirty(token.session_id);
    peer.save(&mut state).unwrap();
    assert_ne!(
        peer.workspace_cleanup_generation(token.session_id).unwrap(),
        Some(token.generation)
    );
    assert!(
        !backend
            .task_store
            .save_workspace_cleanup_run(
                token,
                Some("capture"),
                &CleanupRun {
                    phase: "preserved".into(),
                    payload: "{}".into()
                }
            )
            .unwrap()
    );
    assert_eq!(
        backend
            .task_store
            .workspace_cleanup_run(token)
            .unwrap()
            .unwrap()
            .phase,
        "capture"
    );
    drop(peer);
    drop(backend);

    let (backend, token, source) = fixture(&root, "shared");
    let alias = root.join("shared-alias");
    std::os::unix::fs::symlink(&source, &alias).unwrap();
    let peer = StateStore::daemon(backend.task_store.path().to_owned());
    let mut state = peer.load().unwrap();
    let mut other = AgentSession::new(state.projects[0].id, ProviderKind::Codex);
    other.begin_turn("borrowed owner");
    other.finish_active_turn(TurnStatus::Completed);
    other.archived_at = Some(100);
    other.workspace = SessionWorkspace::Worktree {
        path: alias,
        name: "borrowed".into(),
        branch: None,
        base_branch: None,
        borrowed: true,
        adopted_by: None,
    };
    let other_id = other.id;
    state.sessions.push(other);
    state.mark_session_dirty(other_id);
    peer.save(&mut state).unwrap();
    assert!(
        backend
            .advance_projectless_cleanup(token, &projects, || true)
            .is_err()
    );
    assert!(
        backend
            .task_store
            .workspace_cleanup_run(token)
            .unwrap()
            .is_none()
    );
    assert!(source.join("nested/keep").exists());
    drop(peer);
    drop(backend);

    let (backend, token, source) = fixture(&root, "prior");
    let paths = ArchiveLocations::new(&projects, &source, Uuid::nil()).unwrap();
    paths.prepare_archive_parent().unwrap();
    let mut writer = zip::ZipWriter::new(fs::File::create(&paths.archive).unwrap());
    writer
        .start_file("prior", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"prior valid archive").unwrap();
    writer.finish().unwrap();
    let before = fs::read(&paths.archive).unwrap();
    assert!(
        backend
            .advance_projectless_cleanup(token, &projects, || true)
            .is_err()
    );
    assert_eq!(fs::read(&paths.archive).unwrap(), before);
    assert!(source.join("nested/keep").exists());
    drop(backend);
}
