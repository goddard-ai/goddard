//! Bounded, allocation-free timing counters. Reading or recording these never
//! performs I/O; diagnostics consumers decide where to publish a snapshot.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

#[derive(Clone, Copy)]
pub enum Work {
    Save,
    Catalog,
    Teardown,
    FileWrite,
    SessionPrepare,
}

#[derive(Default)]
struct Counter {
    calls: AtomicU64,
    total_us: AtomicU64,
    max_us: AtomicU64,
    over_frame: AtomicU64,
}

static COUNTERS: [Counter; 5] = [const {
    Counter {
        calls: AtomicU64::new(0),
        total_us: AtomicU64::new(0),
        max_us: AtomicU64::new(0),
        over_frame: AtomicU64::new(0),
    }
}; 5];

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub name: &'static str,
    pub calls: u64,
    pub total_us: u64,
    pub max_us: u64,
    pub over_frame: u64,
}

#[must_use]
pub struct Span {
    work: Work,
    start: Instant,
}

pub fn measure(work: Work) -> Span {
    Span {
        work,
        start: Instant::now(),
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_micros().min(u64::MAX as u128) as u64;
        let counter = &COUNTERS[self.work as usize];
        counter.calls.fetch_add(1, Relaxed);
        counter.total_us.fetch_add(elapsed, Relaxed);
        counter.max_us.fetch_max(elapsed, Relaxed);
        if elapsed >= 8_333 {
            counter.over_frame.fetch_add(1, Relaxed);
        }
    }
}

pub fn snapshot() -> [Timing; 5] {
    let names = [
        "save",
        "catalog",
        "teardown",
        "file_write",
        "session_prepare",
    ];
    std::array::from_fn(|index| {
        let counter = &COUNTERS[index];
        Timing {
            name: names[index],
            calls: counter.calls.load(Relaxed),
            total_us: counter.total_us.load(Relaxed),
            max_us: counter.max_us.load(Relaxed),
            over_frame: counter.over_frame.load(Relaxed),
        }
    })
}
