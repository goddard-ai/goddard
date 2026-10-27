use super::*;
use std::time::Duration;

fn git(path: &Path, args: &[&str]) {
    let result = std::process::Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

struct ReleaseFilter(PathBuf);
impl Drop for ReleaseFilter {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release"), "");
    }
}

#[test]
fn checkpoint_origins_fence_peer_backends_and_keep_each_attempt_independent() {
    for mode in ["success", "receipt_failure", "capture_failure"] {
        let root =
            std::env::temp_dir().join(format!("checkpoint-origin-{mode}-{}", Uuid::new_v4()));
        let (backend, session) = super::super::tests::surface_test_backend(&root);
        let backend = Arc::new(backend);
        let path = root.join("repo");
        git(&path, &["init", "-q"]);
        git(
            &path,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "seed",
            ],
        );
        std::fs::write(
            path.join(".gitattributes"),
            "*.filtered filter=controlled\n",
        )
        .unwrap();
        std::fs::write(path.join("payload.filtered"), "final content\n").unwrap();
        std::fs::write(
            root.join("filter.py"),
            r#"import pathlib, sys, time
root = pathlib.Path(__file__).resolve().parent
data = sys.stdin.buffer.read()
(root / 'entered').touch()
deadline = time.monotonic() + 20
while not (root / 'release').exists():
    if time.monotonic() >= deadline:
        raise SystemExit(41)
    time.sleep(0.005)
if (root / 'fail').exists():
    raise SystemExit(42)
sys.stdout.buffer.write(data)
"#,
        )
        .unwrap();
        git(
            &path,
            &["config", "filter.controlled.clean", "python3 ../filter.py"],
        );
        git(&path, &["config", "filter.controlled.required", "true"]);
        let _release = ReleaseFilter(root.clone());
        let count = backend
            .task_store
            .load_session_detail(session)
            .unwrap()
            .unwrap()
            .turns
            .last()
            .unwrap()
            .turn_count;
        let connection = rusqlite::Connection::open(backend.task_store.path()).unwrap();
        let total_origins = || {
            connection
                .query_row(
                    "SELECT count(*) FROM checkpoint_workspace_writers",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
        };
        // Rejected origin publication must prevent contact with the filter.
        connection.execute_batch("CREATE TRIGGER reject_checkpoint_origin BEFORE INSERT ON checkpoint_workspace_writers BEGIN SELECT RAISE(ABORT, 'fixture origin failure'); END;").unwrap();
        assert!(
            backend
                .capture_turn_checkpoint(path.clone(), session, count, false)
                .is_err()
        );
        assert!(!root.join("entered").exists());
        assert_eq!(total_origins(), 0);
        connection
            .execute_batch("DROP TRIGGER reject_checkpoint_origin;")
            .unwrap();
        if mode == "receipt_failure" {
            connection.execute_batch("CREATE TRIGGER reject_checkpoint_receipt BEFORE UPDATE ON checkpoint_workspace_writers BEGIN SELECT RAISE(ABORT, 'fixture receipt failure'); END;").unwrap();
        }
        if mode == "capture_failure" {
            std::fs::write(root.join("fail"), "").unwrap();
        }
        let capturing = {
            let backend = Arc::clone(&backend);
            let path = path.clone();
            std::thread::spawn(move || backend.capture_turn_checkpoint(path, session, count, false))
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !root.join("entered").exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "controlled Git filter was not reached"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let first = backend.task_store.checkpoint_writer_origins().unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].session_id, session);
        assert_eq!(first[0].turn_count, count);
        let peer = WakuBackend::new(
            DaemonSettingsStore::open(root.join("peer-settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        assert!(
            peer.require_no_observed_workspace_writers(&path)
                .unwrap_err()
                .to_string()
                .contains("durable checkpoint writer")
        );
        std::fs::write(root.join("release"), "").unwrap();
        let result = capturing.join().unwrap();
        if mode == "capture_failure" {
            assert!(result.is_err());
            assert_eq!(
                backend.task_store.checkpoint_writer_origins().unwrap(),
                first
            );
            // A successful retry for the same task/turn has a new physical id.
            git(&path, &["config", "filter.controlled.clean", "cat"]);
            backend
                .capture_turn_checkpoint(path.clone(), session, count, false)
                .unwrap();
            assert_eq!(
                backend.task_store.checkpoint_writer_origins().unwrap(),
                first
            );
            assert_eq!(total_origins(), 2);
            drop(peer);
            drop(backend);
            let reopened = WakuBackend::new(
                DaemonSettingsStore::open(root.join("reopened-settings.json")).unwrap(),
                StateStore::daemon(root.join("app.db")),
            )
            .unwrap();
            assert!(
                reopened
                    .require_no_observed_workspace_writers(&path)
                    .is_err()
            );
            // An identical pathname with a new directory must not hide origin.
            std::fs::rename(&path, root.join("retired-repo")).unwrap();
            std::fs::create_dir(&path).unwrap();
            assert!(
                reopened
                    .require_no_observed_workspace_writers(&path)
                    .unwrap_err()
                    .to_string()
                    .contains("moved, was reused")
            );
            drop(reopened);
        } else {
            assert_eq!(
                result.unwrap().status,
                crate::model::CheckpointStatus::Ready
            );
            let before = total_origins();
            backend
                .capture_turn_checkpoint(path.clone(), session, count, false)
                .unwrap();
            assert_eq!(total_origins(), before, "a deduped read created an origin");
            if mode == "receipt_failure" {
                assert_eq!(
                    backend.task_store.checkpoint_writer_origins().unwrap(),
                    first
                );
                assert!(peer.require_no_observed_workspace_writers(&path).is_err());
                assert!(
                    backend
                        .capture_turn_checkpoint(path.clone(), session, count + 1, false)
                        .is_err()
                );
                assert_eq!(
                    total_origins(),
                    before,
                    "receipt failure admitted a new Git attempt"
                );
                connection
                    .execute_batch("DROP TRIGGER reject_checkpoint_receipt;")
                    .unwrap();
                backend
                    .capture_turn_checkpoint(path.clone(), session, count + 1, false)
                    .unwrap();
                assert!(
                    backend
                        .runtime_lifecycles
                        .checkpoint_confirmations
                        .lock()
                        .is_empty()
                );
            }
            assert!(
                backend
                    .task_store
                    .checkpoint_writer_origins()
                    .unwrap()
                    .is_empty()
            );
            backend
                .require_no_observed_workspace_writers(&path)
                .unwrap();
            peer.require_no_observed_workspace_writers(&path).unwrap();
            drop(peer);
            drop(backend);
        }
        drop(connection);
        drop(_release);
        std::fs::remove_dir_all(root).unwrap();
    }
}
