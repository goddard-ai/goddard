//! Small mandatory control recovery, independent of optional idle maintenance.
//! The worker retains no provider handles or transcript payloads.
use super::*;
use std::sync::atomic::Ordering;
use std::time::Duration;

impl WakuBackend {
    pub(crate) fn start_stable_stop_recovery(&self) {
        if self
            .runtime_lifecycles
            .shutting_down
            .load(Ordering::Acquire)
        {
            return;
        }
        if self
            .stable_stop_recovery_started
            .swap(true, Ordering::AcqRel)
        {
            return;
        }
        let weak = self.weak_self();
        let started = std::thread::Builder::new()
            .name("goddard-stop-recovery".into())
            .spawn(move || {
                loop {
                    let Some(backend) = weak.upgrade() else {
                        return;
                    };
                    if backend
                        .runtime_lifecycles
                        .shutting_down
                        .load(Ordering::Acquire)
                    {
                        return;
                    }
                    let pending = backend.task_store.path().try_exists().and_then(|exists| {
                        if exists {
                            backend.task_store.stable_stops_pending()
                        } else {
                            Ok(false)
                        }
                    });
                    match pending {
                        // Keep polling after an empty queue: an older daemon
                        // may commit its last intent after this one starts.
                        Ok(false) => {}
                        Ok(true) => {
                            if let Err(error) = backend.recover_stable_stops_once() {
                                eprintln!("stable provider recovery deferred: {error}");
                            }
                        }
                        Err(error) => {
                            eprintln!("stable provider recovery storage unavailable: {error}")
                        }
                    }
                    drop(backend);
                    std::thread::sleep(Duration::from_secs(5));
                }
            });
        if let Err(error) = started {
            self.stable_stop_recovery_started
                .store(false, Ordering::Release);
            eprintln!("could not start stable provider recovery: {error}");
        }
    }

    pub(crate) fn recover_stable_stops_once(&self) -> anyhow::Result<()> {
        for record in self
            .task_store
            .stable_stops_due(crate::model::unix_time())?
        {
            if self
                .runtime_lifecycles
                .shutting_down
                .load(Ordering::Acquire)
            {
                break;
            }
            let result = self.with_stable_stop_gate(record.intent.session_id, || {
                if self
                    .runtime_lifecycles
                    .shutting_down
                    .load(Ordering::Acquire)
                {
                    return Ok(());
                }
                crate::stable_runtime::recover_stop(self, &record.intent)
            });
            if let Err(error) = result {
                self.task_store.stable_stop_retry(
                    &record.intent,
                    crate::model::unix_time(),
                    &error.to_string(),
                )?;
            }
        }
        Ok(())
    }
}
