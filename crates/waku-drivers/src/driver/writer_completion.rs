//! A lightweight observation of one writer's lifetime. Retaining this value
//! never retains the driver, subprocess, actor, or event stream.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

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
pub struct WriterCompletion(Arc<AtomicU8>);

impl WriterCompletion {
    /// Launch identity, independent of task/runtime ids supplied by callers.
    pub fn same_writer(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn state(&self) -> WriterCompletionState {
        match self.0.load(Ordering::Acquire) {
            1 => WriterCompletionState::Pending,
            2 => WriterCompletionState::Confirmed,
            _ => WriterCompletionState::Unknown,
        }
    }

    pub(super) fn register(&self) {
        let _ = self
            .0
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
    }

    /// Only the owning adapter may confirm: an exit event, dropped handle,
    /// requested shutdown, or failed process wait is insufficient evidence.
    pub(super) fn confirm(&self) {
        let _ = self
            .0
            .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire);
    }
}
