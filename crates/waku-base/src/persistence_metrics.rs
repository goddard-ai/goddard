//! Bounded process-local SQLite timings, sampled by daemon-stats without per-save I/O.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const WINDOW: usize = 256;
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct WriteSample {
    pub prepare_us: u64,
    pub writer_wait_us: u64,
    pub transaction_us: u64,
    pub commit_us: u64,
    pub total_us: u64,
    pub rows_changed: u64,
    pub success: bool,
}
#[derive(Default)]
struct Lane {
    attempts: u64,
    failures: u64,
    recent: VecDeque<WriteSample>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct PersistenceSample {
    pub lane: String,
    pub attempts: u64,
    pub failures: u64,
    pub recent_count: usize,
    pub total_p95_us: u64,
    pub writer_wait_p95_us: u64,
    pub commit_p95_us: u64,
    pub latest: WriteSample,
}
fn lanes() -> &'static Mutex<BTreeMap<&'static str, Lane>> {
    static LANES: OnceLock<Mutex<BTreeMap<&'static str, Lane>>> = OnceLock::new();
    LANES.get_or_init(Default::default)
}
fn micros(duration: Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}
/// Records failed attempts too. Callers fill phase durations before propagating errors.
pub struct WriteMeasurement {
    lane: &'static str,
    started: Instant,
    pub sample: WriteSample,
}
impl WriteMeasurement {
    pub fn new(lane: &'static str) -> Self {
        Self {
            lane,
            started: Instant::now(),
            sample: WriteSample::default(),
        }
    }
    pub fn prepared(&mut self) {
        self.sample.prepare_us = micros(self.started.elapsed());
    }
    pub fn waited(&mut self, duration: Duration) {
        self.sample.writer_wait_us = micros(duration);
    }
    pub fn worked(&mut self, duration: Duration) {
        self.sample.transaction_us = micros(duration);
    }
    pub fn committed(&mut self, duration: Duration) {
        self.sample.commit_us = micros(duration);
    }
}
impl Drop for WriteMeasurement {
    fn drop(&mut self) {
        self.sample.total_us = micros(self.started.elapsed());
        let mut lanes = lanes().lock().unwrap_or_else(|poison| poison.into_inner());
        let lane = lanes.entry(self.lane).or_default();
        lane.attempts = lane.attempts.saturating_add(1);
        lane.failures = lane
            .failures
            .saturating_add(u64::from(!self.sample.success));
        if lane.recent.len() == WINDOW {
            lane.recent.pop_front();
        }
        lane.recent.push_back(self.sample.clone());
    }
}
/// Lifetime counters plus p95 over the most recent 256 attempts in each lane.
pub fn snapshot() -> Vec<PersistenceSample> {
    let lanes = lanes().lock().unwrap_or_else(|poison| poison.into_inner());
    lanes
        .iter()
        .map(|(name, lane)| {
            let percentile = |field: fn(&WriteSample) -> u64| {
                let mut values = lane.recent.iter().map(field).collect::<Vec<_>>();
                values.sort_unstable();
                values
                    .get((values.len() * 95).div_ceil(100).saturating_sub(1))
                    .copied()
                    .unwrap_or(0)
            };
            PersistenceSample {
                lane: name.to_string(),
                attempts: lane.attempts,
                failures: lane.failures,
                recent_count: lane.recent.len(),
                total_p95_us: percentile(|s| s.total_us),
                writer_wait_p95_us: percentile(|s| s.writer_wait_us),
                commit_p95_us: percentile(|s| s.commit_us),
                latest: lane.recent.back().cloned().unwrap_or_default(),
            }
        })
        .collect()
}
