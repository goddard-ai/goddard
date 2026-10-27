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
