use super::*;
use std::time::{Duration, Instant};

struct HeldRollback {
    started: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
    provider: ProviderKind,
}

impl crate::driver::DriverControl for HeldRollback {
    fn prompt(&self, _: String) {}
    fn cancel(&self) {}
    fn respond(&self, _: String, _: String) {}
    fn rollback(&self, _: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        self.started.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(10)).unwrap();
        Ok(Some(ProviderResumeCursor::from_session_id(
            self.provider,
            "held-provider".into(),
        )))
    }
    fn fork(&self, _: usize) -> anyhow::Result<ProviderResumeCursor> {
        self.rollback(1)?
            .ok_or_else(|| anyhow!("held fixture returned no cursor"))
    }
}

struct ObservedCancel(crossbeam_channel::Sender<()>);

impl crate::driver::DriverControl for ObservedCancel {
    fn prompt(&self, _: String) {}
    fn cancel(&self) {
        self.0.send(()).unwrap();
    }
    fn respond(&self, _: String, _: String) {}
    fn rollback(&self, _: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        Ok(None)
    }
}

#[derive(Clone, Copy)]
enum ResponseOperation {
    Rollback,
    Fork,
    OpenCodeRewind,
    MuseRewind,
}

#[test]
fn response_operations_leave_other_task_controls_responsive_during_rollback() {
    probe_unrelated_cancellation(ResponseOperation::Rollback);
}
#[test]
fn response_operations_leave_other_task_controls_responsive_during_fork() {
    probe_unrelated_cancellation(ResponseOperation::Fork);
}
#[test]
fn response_operations_leave_other_task_controls_responsive_during_opencode_rewind() {
    probe_unrelated_cancellation(ResponseOperation::OpenCodeRewind);
}
#[test]
fn response_operations_leave_other_task_controls_responsive_during_muse_rewind() {
    probe_unrelated_cancellation(ResponseOperation::MuseRewind);
}

fn probe_unrelated_cancellation(operation: ResponseOperation) {
    let root = std::env::temp_dir().join(format!("cancel-during-rewind-{}", Uuid::new_v4()));
    let (backend, source_id) = surface_test_backend(&root);
    let mut source = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|s| s.id == source_id)
        .unwrap()
        .clone();
    source.provider = match operation {
        ResponseOperation::OpenCodeRewind => ProviderKind::OpenCode,
        ResponseOperation::MuseRewind => ProviderKind::Muse,
        _ => ProviderKind::Codex,
    };
    let other_id = Uuid::new_v4();
    let other_runtime = Uuid::new_v4();
    let (started, reached) = crossbeam_channel::bounded(1);
    let (release, blocked) = crossbeam_channel::bounded(1);
    let (cancelled, observed) = crossbeam_channel::unbounded();
    let mut slow =
        runtime_generation_entry(Uuid::new_v4(), Arc::new(CaptureDriver::default()), &root);
    slow.driver = DriverHandle::from_control(Arc::new(HeldRollback {
        started,
        release: blocked,
        provider: source.provider,
    }));
    let mut other =
        runtime_generation_entry(other_runtime, Arc::new(CaptureDriver::default()), &root);
    other.driver = DriverHandle::from_control(Arc::new(ObservedCancel(cancelled)));
    backend.sessions.lock().insert(source_id, slow);
    backend.sessions.lock().insert(other_id, other);
    let backend = Arc::new(backend);
    let rollback = {
        let backend = backend.clone();
        let root = root.clone();
        std::thread::spawn(move || match operation {
            ResponseOperation::Rollback => backend
                .rollback_response_with_driver(&source, &root, &root.join("unused"), 1)
                .map(|_| ()),
            ResponseOperation::Fork => backend
                .fork_response_with_driver(&source, &root, 1)
                .map(|_| ()),
            ResponseOperation::OpenCodeRewind | ResponseOperation::MuseRewind => backend
                .rewind_provider_response(&source, &root, &root.join("unused"), 1, 1, 1, None)
                .map(|_| ()),
        })
    };
    reached.recv_timeout(Duration::from_secs(10)).unwrap();
    let (finished, completion) = crossbeam_channel::bounded(1);
    let probe = {
        let backend = backend.clone();
        std::thread::spawn(move || {
            let mut latency = Vec::new();
            for _ in 0..30 {
                let began = Instant::now();
                backend
                    .handle(
                        Request {
                            request_id: Uuid::new_v4(),
                            session_id: other_id,
                            runtime_id: other_runtime,
                            command: Command::Cancel,
                        },
                        EventSink::detached(),
                        None,
                    )
                    .unwrap();
                latency.push(began.elapsed());
            }
            finished.send(latency).unwrap();
        })
    };
    // Release and join before asserting, even on the baseline's failure.
    let responsive = completion.recv_timeout(Duration::from_millis(250)).ok();
    release.send(()).unwrap();
    rollback.join().unwrap().unwrap();
    probe.join().unwrap();
    let count = observed.try_iter().count();
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(count, 30, "accepted cancellation signals were lost");
    let mut latency = responsive
        .expect("an unrelated Stop waited behind response rollback's shared registry lock");
    latency.sort();
    eprintln!(
        "held response rollback: 30 other-task cancellations p95={:?} max={:?}",
        latency[28], latency[29]
    );
}
