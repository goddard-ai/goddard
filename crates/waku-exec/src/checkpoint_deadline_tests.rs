use super::*;
use std::{
    fs,
    time::{Duration, Instant},
};
#[cfg(unix)]
#[test]
fn checkpoint_listing_has_a_deadline_and_preserves_parsing() {
    use std::os::unix::fs::PermissionsExt;
    let root =
        std::env::temp_dir().join(format!("waku-checkpoint-deadline-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(self.0.join("release"), "");
        }
    }
    let release = Release(root.clone());
    let binary = root.join("shuru");
    fs::write(
        &binary,
        r##"#!/usr/bin/env python3
import os,time
from pathlib import Path
root=Path(__file__).parent
(root/'started').write_text(str(os.getpid()))
if (root/'block').exists():
 while not (root/'release').exists(): time.sleep(.01)
print('NAME SIZE CREATED')
print('waku-provider-codex 123 today')
"##,
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        checkpoint_names(&binary).unwrap(),
        vec!["waku-provider-codex"]
    );
    fs::remove_file(root.join("started")).unwrap();
    fs::write(root.join("block"), "").unwrap();
    let (done, wait) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let start = Instant::now();
        let result = checkpoint_names(&binary);
        let _ = done.send((start.elapsed(), result));
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !root.join("started").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let early = wait.recv_timeout(Duration::from_secs(6));
    let bounded = early.is_ok();
    fs::write(root.join("release"), "").unwrap();
    let (elapsed, result) =
        early.unwrap_or_else(|_| wait.recv_timeout(Duration::from_secs(3)).unwrap());
    worker.join().unwrap();
    let deadline_error = result.as_ref().err().map(|error| format!("{error:#}"));
    eprintln!("checkpoint-proof bounded={bounded} elapsed={elapsed:?} error={deadline_error:?}");
    drop(release);
    fs::remove_dir_all(&root).unwrap();
    assert!(
        bounded && result.is_err(),
        "checkpoint listing ran past its5s execution budget"
    );
    assert!(deadline_error.unwrap().contains("deadline"));
}
