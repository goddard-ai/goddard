//! Readiness must mean persisted state loaded successfully, not just socket bind.
use std::process::Command;

#[test]
fn failed_backend_initialization_does_not_announce_readiness() {
    let root = std::env::temp_dir().join(format!("goddard-readiness-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    // This file loads inside WakuBackend::new, after the listener is bound.
    std::fs::create_dir(root.join("auto-prompts.json")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_goddard-daemon"))
        .env_clear()
        .env("HOME", &root)
        .env("GODDARD_DATA_DIR", &root)
        .env("GODDARD_DEV_DATA_DIR", &root)
        .env("GODDARD_DAEMON_TOKEN", "readiness-test-token")
        .args(["--bind", "127.0.0.1:0"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut timed_out = false;
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            timed_out = true;
            child.kill().unwrap();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !timed_out,
        "daemon did not reject unreadable state: {stderr}"
    );
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("could not load Goddard auto prompt history"),
        "{stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "failed startup announced readiness"
    );
}
