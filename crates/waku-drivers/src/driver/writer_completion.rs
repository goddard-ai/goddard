//! A lightweight observation of one writer's lifetime. Completion observers
//! retain only metadata/persistence handles and weak lifecycle references.
//! Never capture the witness, driver, subprocess, actor, or event stream in an
//! observer: that could retain owned work or create a reference cycle.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriterCompletionState {
    /// The transport has not qualified a positive completion observation.
    Unknown,
    /// An owned writer was registered and has not confirmed termination.
    Pending,
    /// Its owner confirmed termination and the required intake/read drain.
    Confirmed,
}

#[derive(Clone, Default)]
pub struct WriterCompletion(Arc<Completion>);

#[derive(Default)]
struct Completion {
    state: AtomicU8,
    observers: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
}

impl WriterCompletion {
    /// Physical writer identity, independent of task/runtime attempt ids.
    pub fn same_writer(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn state(&self) -> WriterCompletionState {
        match self.0.state.load(Ordering::Acquire) {
            1 => WriterCompletionState::Pending,
            2 => WriterCompletionState::Confirmed,
            _ => WriterCompletionState::Unknown,
        }
    }

    /// Register only when the adapter has identified the physical writer.
    /// External transports may share this witness across reattachments.
    pub fn register(&self) {
        let _ = self
            .0
            .state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
    }

    /// Only the owning adapter may confirm: an exit event, dropped handle,
    /// requested shutdown, or failed process wait is insufficient evidence.
    pub fn confirm(&self) {
        if self
            .0
            .state
            .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let observers =
                std::mem::take(&mut *self.0.observers.lock().expect("writer observers poisoned"));
            for observer in observers {
                observer();
            }
        }
    }

    /// Observe this exact writer's qualified completion once. Late observers
    /// run immediately; an unregistered writer or a dropped handle never fires.
    /// Callbacks run outside the observer lock on the confirming/observing thread.
    pub fn on_confirm(&self, observer: impl FnOnce() + Send + 'static) {
        let mut observers = self.0.observers.lock().expect("writer observers poisoned");
        if self.state() == WriterCompletionState::Confirmed {
            drop(observers);
            observer();
        } else {
            observers.push(Box::new(observer));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn qualified_completion_notifies_racing_and_late_observers_once() {
        let witness = WriterCompletion::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        witness.on_confirm(move || {
            count.fetch_add(1, Ordering::Relaxed);
        });
        witness.confirm();
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        witness.register();
        let barrier = Arc::new(std::sync::Barrier::new(9));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let witness = witness.clone();
                let calls = Arc::clone(&calls);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    witness.on_confirm(move || {
                        calls.fetch_add(1, Ordering::Relaxed);
                    });
                })
            })
            .collect();
        barrier.wait();
        witness.confirm();
        for worker in workers {
            worker.join().unwrap();
        }
        let count = Arc::clone(&calls);
        witness.on_confirm(move || {
            count.fetch_add(1, Ordering::Relaxed);
        });
        witness.confirm();
        drop(witness);
        assert_eq!(calls.load(Ordering::Relaxed), 10);
    }
}
