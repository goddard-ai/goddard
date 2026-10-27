use super::*;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use uuid::Uuid;

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_until(child: &mut Child, ready: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "admission child exited before readiness"
        );
        assert!(
            Instant::now() < deadline,
            "admission child did not become ready"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

// A concurrent process spawn can briefly inherit another test's descriptors
// before CLOEXEC closes them. Production acquisition deliberately defers rather
// than waits; release assertions allow that handoff, but still fail on a leak.
fn retirement_after_release(store: &Path, path: &Path) -> WorkspaceAdmission {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match WorkspaceAdmission::retirement(store, path) {
            Ok(guard) => return guard,
            Err(error) => {
                assert!(
                    error
                        .to_string()
                        .starts_with("workspace admission deferred:"),
                    "{error:#}"
                );
                assert!(
                    Instant::now() < deadline,
                    "released admission remained locked: {error:#}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

#[test]
fn admission_is_cross_process_and_releases_without_a_persistent_owner() {
    const CHILD_ROOT: &str = "GODDARD_ADMISSION_FIXTURE_ROOT";
    const CHILD_MODE: &str = "GODDARD_ADMISSION_FIXTURE_MODE";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = PathBuf::from(root);
        let store = root.join("app.db");
        let nested = root.join("workspace/nested");
        let _guard = if std::env::var(CHILD_MODE).unwrap() == "writer" {
            WorkspaceAdmission::writer(&store, &nested)
        } else {
            WorkspaceAdmission::retirement(&store, &nested)
        }
        .unwrap();
        fs::write(root.join("ready"), b"ready").unwrap();
        // Parent owns this pipe. Normal release and process death both close
        // our lock descriptors; neither invents physical writer completion.
        std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
        return;
    }
    let root = std::env::temp_dir().join(format!("admission-process-{}", Uuid::new_v4()));
    let workspace = root.join("workspace");
    let nested = workspace.join("nested");
    let descendant = nested.join("deeper");
    let unrelated = root.join("unrelated");
    fs::create_dir_all(&descendant).unwrap();
    fs::create_dir_all(&unrelated).unwrap();
    fs::write(root.join("app.db"), b"fixture").unwrap();
    let store = root.join("app.db");
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&nested, &alias).unwrap();
    for mode in ["writer", "retirement"] {
        let ready = root.join("ready");
        let mut child = ChildGuard(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "workspace_admission::tests::admission_is_cross_process_and_releases_without_a_persistent_owner", "--nocapture"])
            .env(CHILD_ROOT, &root)
            .env(CHILD_MODE, mode)
            .stdin(Stdio::piped())
            .spawn().unwrap());
        wait_until(&mut child.0, &ready);
        for path in [&workspace, &alias, &descendant] {
            assert!(WorkspaceAdmission::retirement(&store, path).is_err());
            if mode == "retirement" {
                assert!(WorkspaceAdmission::writer(&store, path).is_err());
            }
        }
        drop(WorkspaceAdmission::writer(&store, &unrelated).unwrap());
        if mode == "writer" {
            drop(WorkspaceAdmission::writer(&store, &nested).unwrap());
            drop(child.0.stdin.take());
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                assert!(Instant::now() < deadline, "admission child did not exit");
                std::thread::sleep(Duration::from_millis(5));
            }
        } else {
            // An abandoned retirement reservation cannot pin admission forever.
            child.0.kill().unwrap();
            child.0.wait().unwrap();
        }
        drop(child);
        fs::remove_file(ready).unwrap();
        drop(WorkspaceAdmission::retirement(&store, &nested).unwrap());
        drop(WorkspaceAdmission::writer(&store, &nested).unwrap());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn untrusted_gate_files_fail_closed_and_failed_acquisition_releases_earlier_locks() {
    let root = std::env::temp_dir().join(format!("admission-files-{}", Uuid::new_v4()));
    let nested = root.join("workspace/nested");
    fs::create_dir_all(&nested).unwrap();
    let store = root.join("app.db");
    fs::write(&store, b"fixture").unwrap();
    let guard = WorkspaceAdmission::writer(&store, &nested).unwrap();
    // Acquisition fails after obtaining its first own-path lock. All partial
    // guards must release so that a subsequent writer can still publish.
    assert!(WorkspaceAdmission::retirement(&store, &root).is_err());
    drop(guard);
    drop(retirement_after_release(&store, &root));

    let gate = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".workspace-admission-")
        })
        .unwrap();
    use sha2::{Digest, Sha256};
    use std::os::unix::ffi::OsStrExt;
    let lock = gate.join(format!(
        "{:x}-own",
        Sha256::digest(dunce::canonicalize(&nested).unwrap().as_os_str().as_bytes())
    ));
    fs::remove_file(&lock).unwrap();
    std::os::unix::fs::symlink(&store, &lock).unwrap();
    assert!(WorkspaceAdmission::retirement(&store, &nested).is_err());
    assert_eq!(fs::read(&store).unwrap(), b"fixture");
    fs::remove_file(&lock).unwrap();
    drop(retirement_after_release(&store, &nested));
    fs::remove_dir_all(&gate).unwrap();
    std::os::unix::fs::symlink(&nested, &gate).unwrap();
    assert!(WorkspaceAdmission::writer(&store, &nested).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn catalog_guards_missing_paths_and_coalesces_large_batches() {
    let root = std::env::temp_dir().join(format!("admission-catalog-{}", Uuid::new_v4()));
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let store = root.join("app.db");
    fs::write(&store, b"fixture").unwrap();
    let paths = (0..2_000)
        .map(|number| workspace.join(format!("not-created-{number}/nested")))
        .collect::<Vec<_>>();
    let guard = WorkspaceAdmission::catalog(&store, &paths)
        .unwrap()
        .unwrap();
    assert!(
        guard._locks.len() <= 129,
        "catalog batch retained per-row descriptors"
    );
    assert!(WorkspaceAdmission::retirement(&store, &workspace).is_err());
    drop(guard);
    let retirement = retirement_after_release(&store, &workspace);
    assert!(WorkspaceAdmission::catalog(&store, &paths[..1]).is_err());
    drop(retirement);
    let alias = workspace.join("dangling");
    std::os::unix::fs::symlink(root.join("missing"), &alias).unwrap();
    assert!(WorkspaceAdmission::catalog(&store, &[alias.join("child")]).is_err());
    fs::remove_dir_all(root).unwrap();
}
