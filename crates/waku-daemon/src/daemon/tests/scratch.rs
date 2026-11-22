use super::*;
use waku_protocol::scratch::ScratchOperation;
use waku_protocol::workspace::WorkspaceResult;

fn get(backend: &WakuBackend, task: Uuid, name: Option<&str>) -> PathBuf {
    let ResponsePayload::AgentScratch { path, .. } = backend
        .agent_scratch(
            Some(task),
            Uuid::nil(),
            ScratchOperation::Get {
                name: name.map(str::to_owned),
            },
        )
        .unwrap()
    else {
        panic!("scratch get must return AgentScratch");
    };
    path
}

#[test]
fn agent_scratch_creates_folders_under_the_root_tasks_pool() {
    let root = std::env::temp_dir().join(format!("waku-scratch-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let pool = root.join("scratch").join(session_id.to_string());

    // The bare form returns the task's pool root, created on demand and
    // surviving restarts inside the daemon's data dir.
    assert_eq!(get(&backend, session_id, None), pool);
    assert!(pool.is_dir());

    // Named folders list in sorted order.
    get(&backend, session_id, Some("b-cache"));
    get(&backend, session_id, Some("a-fixtures"));
    let ResponsePayload::AgentScratch { path, entries } = backend
        .agent_scratch(Some(session_id), Uuid::nil(), ScratchOperation::List)
        .unwrap()
    else {
        panic!("scratch list must return AgentScratch");
    };
    assert_eq!(path, pool);
    let names: Vec<_> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["a-fixtures", "b-cache"]);
    assert!(
        entries
            .iter()
            .all(|entry| entry.path.parent() == Some(pool.as_path()))
    );

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn scratch_names_cannot_escape_the_pool() {
    let root = std::env::temp_dir().join(format!("waku-scratch-names-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let long = "x".repeat(65);
    for name in [
        "",
        ".",
        "..",
        "../escape",
        "nested/dir",
        "/absolute",
        "has space",
        "tab\tname",
        long.as_str(),
    ] {
        let error = backend
            .agent_scratch(
                Some(session_id),
                Uuid::nil(),
                ScratchOperation::Get {
                    name: Some(name.to_owned()),
                },
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("invalid scratch name"),
            "{name:?} must reject: {error}"
        );
    }
    // Nothing leaked outside the pool or into it.
    assert!(!root.join("scratch").exists());

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_side_chat_shares_its_parent_tasks_scratch_pool() {
    let root = std::env::temp_dir().join(format!("waku-scratch-side-{}", Uuid::new_v4()));
    let (backend, session_id, side_id) = read_scope_test_backend(&root);
    let pool = root.join("scratch").join(session_id.to_string());

    assert_eq!(get(&backend, side_id, Some("shared")), pool.join("shared"));
    // …and the parent sees what the side chat made.
    let ResponsePayload::AgentScratch { entries, .. } = backend
        .agent_scratch(Some(session_id), Uuid::nil(), ScratchOperation::List)
        .unwrap()
    else {
        panic!("scratch list must return AgentScratch");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "shared");
    // The side chat owns no pool of its own.
    assert!(!root.join("scratch").join(side_id.to_string()).exists());

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn unknown_or_archived_tasks_get_no_scratch() {
    let root = std::env::temp_dir().join(format!("waku-scratch-dead-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);

    let unknown = Uuid::new_v4();
    let error = backend
        .agent_scratch(
            Some(unknown),
            Uuid::nil(),
            ScratchOperation::Get { name: None },
        )
        .unwrap_err();
    assert!(error.to_string().contains("unknown task"));

    backend.archive_sessions(&[session_id]).unwrap();
    let error = backend
        .agent_scratch(
            Some(session_id),
            Uuid::nil(),
            ScratchOperation::Get { name: None },
        )
        .unwrap_err();
    assert!(error.to_string().contains("archived"));

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn removing_a_task_drops_its_scratch_pool() {
    let root = std::env::temp_dir().join(format!("waku-scratch-remove-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let pool = get(&backend, session_id, None);
    get(&backend, session_id, Some("work"));
    std::fs::write(pool.join("work").join("data.bin"), b"payload").unwrap();

    backend.remove_session(session_id).unwrap();
    assert!(!pool.exists(), "the whole pool leaves with the task");

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn archiving_a_task_drops_its_scratch_pool_immediately() {
    let root = std::env::temp_dir().join(format!("waku-scratch-archive-{}", Uuid::new_v4()));
    let (backend, session_id, side_id) = read_scope_test_backend(&root);
    // Reproducible by contract, so scratch ignores the worktree's
    // archive-retention window — the pool dies with the flag, not the
    // purge, and the side chat's writes share that fate.
    get(&backend, side_id, Some("scratch"));
    let pool = root.join("scratch").join(session_id.to_string());

    backend.archive_sessions(&[session_id]).unwrap();
    assert!(!pool.exists());

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn removing_a_project_drops_every_tasks_scratch_pool() {
    let root = std::env::temp_dir().join(format!("waku-scratch-project-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let pool = get(&backend, session_id, None);
    let project_id = backend.task_state.lock().sessions[0].project_id;

    backend.remove_project(project_id).unwrap();
    assert!(!pool.exists());

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn inspect_and_reclaim_task_scratch_measure_then_delete_the_pool() {
    let root = std::env::temp_dir().join(format!("waku-scratch-reclaim-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let pool = get(&backend, session_id, None);
    std::fs::write(pool.join("blob.bin"), vec![0u8; 1024]).unwrap();

    let WorkspaceResult::TaskScratch { path, bytes } = backend.inspect_task_scratch(session_id)
    else {
        panic!("inspect must return TaskScratch");
    };
    assert_eq!(path, pool);
    assert!(bytes >= 1024);

    let WorkspaceResult::Reclaim {
        reclaimed_bytes,
        failures,
    } = backend.reclaim_task_scratch(session_id)
    else {
        panic!("reclaim must return Reclaim");
    };
    assert_eq!(reclaimed_bytes, bytes);
    assert!(failures.is_empty());
    assert!(!pool.exists());

    // Idempotent: a second pass frees nothing and fails nothing.
    let WorkspaceResult::Reclaim {
        reclaimed_bytes,
        failures,
    } = backend.reclaim_task_scratch(session_id)
    else {
        panic!("reclaim must return Reclaim");
    };
    assert_eq!(reclaimed_bytes, 0);
    assert!(failures.is_empty());

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn stale_or_dead_owners_have_nothing_to_reclaim() {
    let root = std::env::temp_dir().join(format!("waku-scratch-stale-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);

    // An unknown session resolves to no owner — empty inspection, empty
    // reclaim, nothing deleted anywhere.
    let unknown = Uuid::new_v4();
    let WorkspaceResult::TaskScratch { path, bytes } = backend.inspect_task_scratch(unknown) else {
        panic!("inspect must return TaskScratch");
    };
    assert!(path.as_os_str().is_empty() && bytes == 0);
    let WorkspaceResult::Reclaim {
        reclaimed_bytes,
        failures,
    } = backend.reclaim_task_scratch(unknown)
    else {
        panic!("reclaim must return Reclaim");
    };
    assert_eq!(reclaimed_bytes, 0);
    assert!(failures.is_empty());

    // An archived owner is just as dead to reclaim even when its pool
    // still sits on disk — the sweep, not the client request, collects
    // it. (Mirrors a crash between the archive flag and the unlink.)
    let pool = root.join("scratch").join(session_id.to_string());
    std::fs::create_dir_all(&pool).unwrap();
    std::fs::write(pool.join("left.bin"), b"x").unwrap();
    backend.task_state.lock().sessions[0].archived_at = Some(crate::model::unix_time());
    let WorkspaceResult::Reclaim {
        reclaimed_bytes, ..
    } = backend.reclaim_task_scratch(session_id)
    else {
        panic!("reclaim must return Reclaim");
    };
    assert_eq!(reclaimed_bytes, 0);
    assert!(pool.exists(), "reclaim refuses a dead owner…");
    backend.sweep_scratch();
    assert!(!pool.exists(), "…and the sweep collects it");

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn the_sweep_keeps_live_pools_and_leaves_foreign_entries_alone() {
    let root = std::env::temp_dir().join(format!("waku-scratch-sweep-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let scratch = root.join("scratch");
    let live = get(&backend, session_id, None);

    // Orphaned pool — its owner was never a task here.
    let orphan = scratch.join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&orphan).unwrap();
    // An archived owner's leftover pool.
    let dead = scratch.join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dead).unwrap();
    // Foreign entries: a non-UUID directory and a UUID-named file.
    let foreign_dir = scratch.join("not-a-task");
    std::fs::create_dir_all(&foreign_dir).unwrap();
    let uuid_file = scratch.join(Uuid::new_v4().to_string());
    std::fs::write(&uuid_file, b"marker").unwrap();

    backend.sweep_scratch();
    assert!(live.exists());
    assert!(!orphan.exists());
    assert!(!dead.exists());
    assert!(foreign_dir.exists());
    assert!(uuid_file.exists());

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}

#[cfg(target_os = "macos")]
#[test]
fn the_scratch_root_is_excluded_from_time_machine() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let root = std::env::temp_dir().join(format!("waku-scratch-xattr-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);

    get(&backend, session_id, None);
    let path = CString::new(root.join("scratch").as_os_str().as_bytes()).unwrap();
    let name = CString::new("com.apple.metadata:com_apple_backup_excludeItem").unwrap();
    // SAFETY: both C strings are NUL-terminated and outlive the call.
    let size =
        unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) };
    assert!(size > 0, "scratch/ must carry the backup-exclusion xattr");

    drop(backend);
    std::fs::remove_dir_all(root).ok();
}
