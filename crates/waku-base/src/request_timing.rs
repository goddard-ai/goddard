//! In-memory phase timing for synchronous daemon request workers.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;

thread_local! {
    static CAPTURE: RefCell<Option<PhaseCapture>> = const { RefCell::new(None) };
}

#[derive(Clone, Default)]
pub struct PhaseCapture(Arc<Mutex<Phases>>);

#[derive(Default)]
struct Phases {
    current: Option<(&'static str, Instant)>,
    completed_ms: BTreeMap<&'static str, u64>,
}

impl PhaseCapture {
    pub fn run<T>(&self, work: impl FnOnce() -> T) -> T {
        struct Restore(Option<PhaseCapture>);
        impl Drop for Restore {
            fn drop(&mut self) {
                CAPTURE.with(|slot| slot.replace(self.0.take()));
            }
        }

        *self.0.lock() = Phases::default();
        let _restore = Restore(CAPTURE.with(|slot| slot.replace(Some(self.clone()))));
        work()
    }

    pub fn active(&self) -> Option<(&'static str, u64)> {
        self.0
            .lock()
            .current
            .map(|(phase, at)| (phase, at.elapsed().as_millis() as u64))
    }

    pub fn take_completed(&self) -> BTreeMap<String, u64> {
        let completed = std::mem::take(&mut self.0.lock().completed_ms);
        completed
            .into_iter()
            .map(|(name, ms)| (name.to_owned(), ms))
            .collect()
    }
}

/// Time a named phase when running on an instrumented request worker.
pub fn phase<T>(name: &'static str, work: impl FnOnce() -> T) -> T {
    struct Finish {
        capture: PhaseCapture,
        previous: Option<(&'static str, Instant)>,
    }

    impl Drop for Finish {
        fn drop(&mut self) {
            let mut phases = self.capture.0.lock();
            let (name, started) = std::mem::replace(&mut phases.current, self.previous)
                .expect("a phase guard owns the active phase");
            let elapsed = started.elapsed().as_millis() as u64;
            if elapsed > 0 {
                *phases.completed_ms.entry(name).or_default() += elapsed;
            }
        }
    }

    let _finish = CAPTURE.with(|slot| {
        let capture = slot.borrow().clone()?;
        let previous = capture.0.lock().current.replace((name, Instant::now()));
        Some(Finish { capture, previous })
    });
    work()
}
