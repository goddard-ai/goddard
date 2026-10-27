//! Plain background results keep worker reclamation through abandoned UI handoffs.

/// Own a Send payload until it is accepted. Dropping an abandoned wrapper
/// reclaims its payload on the blocking pool, including when a weak UI update
/// never runs. Call into_inner only at the actual ownership-transfer boundary.
pub struct BackgroundOwned<T: Send + 'static>(Option<T>);

impl<T: Send + 'static> BackgroundOwned<T> {
    pub fn new(value: T) -> Self {
        Self(Some(value))
    }

    pub fn get(&self) -> &T {
        self.0.as_ref().expect("background payload was consumed")
    }

    pub fn into_inner(mut self) -> T {
        self.0.take().expect("background payload was consumed")
    }
}

impl<T: Send + 'static> Drop for BackgroundOwned<T> {
    fn drop(&mut self) {
        if let Some(value) = self.0.take() {
            super::task_writer::retire(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::thread::{self, ThreadId};
    use std::time::Duration;

    struct ObservedDrop {
        dropped: crossbeam_channel::Sender<ThreadId>,
        count: Arc<AtomicUsize>,
    }
    impl Drop for ObservedDrop {
        fn drop(&mut self) {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.dropped.send(thread::current().id()).unwrap();
        }
    }

    #[test]
    fn abandoned_handoff_reclaims_on_worker_once_and_acceptance_transfers_ownership() {
        let owner = thread::current().id();
        let (dropped, observed) = crossbeam_channel::unbounded();
        let count = Arc::new(AtomicUsize::new(0));
        let abandoned = BackgroundOwned::new(ObservedDrop {
            dropped: dropped.clone(),
            count: count.clone(),
        });
        // A failed weak update discards its closure without entering it.
        let callback = move || abandoned.into_inner();
        drop(callback);
        assert_ne!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            owner
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let accepted = BackgroundOwned::new(ObservedDrop {
            dropped,
            count: count.clone(),
        })
        .into_inner();
        drop(accepted);
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            owner
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(observed.try_recv().is_err());
    }
}
