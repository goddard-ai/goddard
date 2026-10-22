//! Demand-driven sidebar status. Only rendered rows and the selected task
//! claim work; invalidation never queues hidden session history.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use waku_client::DaemonKey;
use waku_protocol::git::CheckoutStatus;

const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const RETRY_LIMIT: Duration = Duration::from_secs(300);
const MAX_RUNNING: usize = 2;
const CACHE_CAPACITY: usize = 512;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct CheckoutKey {
    pub owner: DaemonKey,
    pub path: PathBuf,
    pub base: Option<String>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct DemandMetrics {
    pub cached: usize,
    pub deferred: usize,
    pub retries: usize,
}

struct Entry {
    value: Option<CheckoutStatus>,
    generation: u64,
    running: bool,
    deferred: bool,
    attempted: bool,
    refresh_at: Instant,
    failures: u32,
    last_used: u64,
}

#[derive(Default)]
pub(super) struct CheckoutStatuses {
    entries: HashMap<CheckoutKey, Entry>,
    running: usize,
    clock: u64,
    metrics: DemandMetrics,
}

impl CheckoutStatuses {
    pub fn peek(&self, key: &CheckoutKey) -> Option<CheckoutStatus> {
        self.entries.get(key).and_then(|entry| entry.value)
    }

    /// Claim one slot without queueing any work when all slots are occupied.
    /// A completion notifies the view, allowing still-visible rows to retry.
    pub fn claim(&mut self, key: &CheckoutKey, now: Instant) -> Option<u64> {
        self.clock = self.clock.wrapping_add(1);
        let previous_use = self
            .entries
            .get(key)
            .filter(|entry| entry.attempted)
            .map(|entry| entry.last_used);
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used = self.clock;
            if entry.running || now < entry.refresh_at {
                self.metrics.cached += 1;
                return None;
            }
            // Only capacity-blocked demand gets priority. A fairness yield
            // must not make two due cached rows keep yielding to each other.
            entry.deferred = false;
        }
        if !self.entries.contains_key(key) && self.entries.len() >= CACHE_CAPACITY {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, entry)| !entry.running)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        // A slow successful row must not repeatedly take the next slot ahead
        // of rows requested later in the previous paint. Remembering demand
        // does not queue it: only a row rendered again can claim the slot.
        let yield_to_later_demand = previous_use.is_some_and(|previous| {
            self.entries.iter().any(|(other_key, entry)| {
                other_key != key
                    && !entry.running
                    && entry.deferred
                    && entry.refresh_at <= now
                    && entry.last_used > previous
            })
        });
        let entry = self.entries.entry(key.clone()).or_insert(Entry {
            value: None,
            generation: 0,
            running: false,
            deferred: false,
            attempted: false,
            refresh_at: now,
            failures: 0,
            last_used: self.clock,
        });
        if self.running >= MAX_RUNNING {
            entry.deferred = true;
            self.metrics.deferred += 1;
            return None;
        }
        if yield_to_later_demand {
            self.metrics.deferred += 1;
            return None;
        }
        if entry.failures > 0 {
            self.metrics.retries += 1;
        }
        entry.running = true;
        entry.attempted = true;
        self.running += 1;
        Some(entry.generation)
    }

    /// Changes in one checkout do not refresh any other checkout. Failed
    /// requests retain their backoff even during a stream of tool events.
    pub fn invalidate(&mut self, owner: DaemonKey, path: &Path, now: Instant) {
        for (key, entry) in &mut self.entries {
            if key.owner == owner && key.path == path {
                entry.generation = entry.generation.wrapping_add(1);
                if entry.failures == 0 {
                    entry.refresh_at = now;
                }
            }
        }
    }

    pub fn finish(
        &mut self,
        key: &CheckoutKey,
        generation: u64,
        result: Result<Option<CheckoutStatus>, ()>,
        now: Instant,
    ) {
        let Some(entry) = self.entries.get_mut(key) else {
            return;
        };
        if !entry.running {
            return;
        }
        entry.running = false;
        self.running -= 1;
        match result {
            Ok(value) => {
                entry.failures = 0;
                if entry.generation == generation {
                    entry.value = value;
                    // Confirmed non-repositories are periodically checked for
                    // restoration, without being retried on each repaint.
                    entry.refresh_at = now
                        + if value.is_some() {
                            REFRESH_INTERVAL
                        } else {
                            RETRY_LIMIT
                        };
                }
            }
            Err(()) => {
                entry.failures = entry.failures.saturating_add(1);
                let multiplier = 1_u32 << entry.failures.saturating_sub(1).min(4);
                entry.refresh_at = now + (REFRESH_INTERVAL * multiplier).min(RETRY_LIMIT);
            }
        }
    }

    pub fn take_metrics(&mut self) -> DemandMetrics {
        std::mem::take(&mut self.metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(index: usize) -> CheckoutKey {
        CheckoutKey {
            owner: DaemonKey::Local,
            path: PathBuf::from(format!("checkout-{index}")),
            base: None,
        }
    }

    fn dirty() -> CheckoutStatus {
        CheckoutStatus {
            uncommitted_changes: true,
            unpushed_commits: 1,
        }
    }

    #[test]
    fn hidden_history_never_queues_and_visible_requests_are_bounded() {
        let mut cache = CheckoutStatuses::default();
        let now = Instant::now();
        // Session existence and invalidations alone do not create demand.
        for index in 0..1300 {
            cache.invalidate(DaemonKey::Local, &key(index).path, now);
        }
        assert!(cache.entries.is_empty());
        for index in 0..12 {
            cache.claim(&key(index), now);
        }
        assert_eq!(cache.running, MAX_RUNNING);
        assert_eq!(cache.entries.len(), 12);
        cache.finish(&key(0), 0, Ok(Some(dirty())), now);
        assert_eq!(cache.claim(&key(2), now), Some(0));
        assert_eq!(cache.running, MAX_RUNNING);
    }

    #[test]
    fn invalidation_keeps_one_running_request_and_rejects_stale_answers() {
        let mut cache = CheckoutStatuses::default();
        let now = Instant::now();
        let first = cache.claim(&key(0), now).unwrap();
        for _ in 0..100 {
            cache.invalidate(DaemonKey::Local, &key(0).path, now);
        }
        assert_eq!(cache.claim(&key(0), now), None);
        cache.finish(&key(0), first, Ok(Some(dirty())), now);
        assert_eq!(cache.peek(&key(0)), None);
        let next = cache.claim(&key(0), now).unwrap();
        cache.finish(&key(0), next, Ok(Some(dirty())), now);
        assert_eq!(cache.peek(&key(0)), Some(dirty()));
        assert_eq!(cache.claim(&key(0), now), None);
    }

    #[test]
    fn failures_back_off_without_erasing_last_known_status() {
        let mut cache = CheckoutStatuses::default();
        let now = Instant::now();
        let generation = cache.claim(&key(0), now).unwrap();
        cache.finish(&key(0), generation, Ok(Some(dirty())), now);
        let later = now + REFRESH_INTERVAL;
        let generation = cache.claim(&key(0), later).unwrap();
        cache.finish(&key(0), generation, Err(()), later);
        cache.invalidate(DaemonKey::Local, &key(0).path, later);
        assert_eq!(cache.claim(&key(0), later), None);
        assert_eq!(cache.peek(&key(0)), Some(dirty()));
        assert!(cache.claim(&key(0), later + REFRESH_INTERVAL).is_some());
    }

    #[test]
    fn missing_checkouts_retry_for_restoration_and_keys_include_host_and_base() {
        let mut cache = CheckoutStatuses::default();
        let now = Instant::now();
        let generation = cache.claim(&key(0), now).unwrap();
        cache.finish(&key(0), generation, Ok(None), now);
        assert_eq!(cache.claim(&key(0), now + REFRESH_INTERVAL), None);
        let mut other_base = key(0);
        other_base.base = Some("dev".to_owned());
        let generation = cache.claim(&other_base, now).unwrap();
        cache.finish(&other_base, generation, Ok(Some(dirty())), now);
        assert_eq!(cache.peek(&key(0)), None);
        assert_eq!(cache.peek(&other_base), Some(dirty()));
        let mut other_host = other_base.clone();
        other_host.owner = DaemonKey::Remote(uuid::Uuid::new_v4());
        assert_eq!(cache.peek(&other_host), None);
        cache.invalidate(other_host.owner, &other_host.path, now);
        assert_eq!(cache.claim(&other_base, now), None);
        let generation = cache.claim(&key(0), now + RETRY_LIMIT).unwrap();
        cache.finish(&key(0), generation, Ok(Some(dirty())), now + RETRY_LIMIT);
        assert_eq!(cache.peek(&key(0)), Some(dirty()));
    }

    #[test]
    fn slow_periodic_refreshes_yield_to_waiting_rows() {
        let mut cache = CheckoutStatuses::default();
        let now = Instant::now();
        let first = cache.claim(&key(0), now).unwrap();
        let second = cache.claim(&key(1), now).unwrap();
        assert_eq!(cache.claim(&key(2), now), None);
        cache.finish(&key(0), first, Ok(Some(dirty())), now);
        // By the time the second request completes, the first is due again.
        let later = now + REFRESH_INTERVAL;
        cache.finish(&key(1), second, Ok(Some(dirty())), later);
        assert_eq!(cache.claim(&key(0), later), None);
        assert!(cache.claim(&key(2), later).is_some());
    }

    #[test]
    fn due_cached_rows_do_not_keep_yielding_to_each_other() {
        let mut cache = CheckoutStatuses::default();
        let now = Instant::now();
        for index in 0..MAX_RUNNING {
            let generation = cache.claim(&key(index), now).unwrap();
            cache.finish(&key(index), generation, Ok(Some(dirty())), now);
        }
        let later = now + REFRESH_INTERVAL;
        for index in 0..MAX_RUNNING {
            assert!(cache.claim(&key(index), later).is_some());
        }
        assert_eq!(cache.running, MAX_RUNNING);
    }

    #[test]
    fn hidden_deferred_demand_only_delays_a_visible_row_once() {
        let mut cache = CheckoutStatuses::default();
        let now = Instant::now();
        let first = cache.claim(&key(0), now).unwrap();
        let second = cache.claim(&key(1), now).unwrap();
        assert_eq!(cache.claim(&key(2), now), None);
        cache.finish(&key(0), first, Ok(Some(dirty())), now);
        cache.finish(&key(1), second, Ok(Some(dirty())), now);
        // Row 2 has left the viewport. Its old demand can yield one pass,
        // but cannot reserve capacity or keep a visible row stale forever.
        let later = now + REFRESH_INTERVAL;
        assert_eq!(cache.claim(&key(0), later), None);
        assert!(cache.claim(&key(0), later).is_some());
        assert_eq!(cache.running, 1);
        assert!(!cache.entries.get(&key(0)).unwrap().deferred);
    }

    #[test]
    fn invalidated_first_load_yields_to_deferred_rows() {
        let mut cache = CheckoutStatuses::default();
        let now = Instant::now();
        let first = cache.claim(&key(0), now).unwrap();
        cache.claim(&key(1), now).unwrap();
        assert_eq!(cache.claim(&key(2), now), None);
        cache.invalidate(DaemonKey::Local, &key(0).path, now);
        cache.finish(&key(0), first, Ok(Some(dirty())), now);
        assert_eq!(cache.peek(&key(0)), None);
        assert_eq!(cache.claim(&key(0), now), None);
        assert!(cache.claim(&key(2), now).is_some());
    }

    #[test]
    fn scrolling_through_history_keeps_cache_bounded() {
        let mut cache = CheckoutStatuses::default();
        let now = Instant::now();
        for index in 0..1300 {
            let generation = cache.claim(&key(index), now).unwrap();
            cache.finish(&key(index), generation, Ok(Some(dirty())), now);
        }
        assert_eq!(cache.entries.len(), CACHE_CAPACITY);
        assert_eq!(cache.peek(&key(0)), None);
        assert_eq!(cache.peek(&key(1299)), Some(dirty()));
    }
}
