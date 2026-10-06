//! A process-wide bound on concurrent bounded-duration subprocesses.
//!
//! Request pools cap how many *requests* run at once, but what actually
//! hurts a loaded machine is how many children are alive at once — a dozen
//! `git` invocations or provider probes each fork and fight over the disk.
//! `command_env` routes every bounded-duration spawn (`Proc::output`,
//! `Proc::status`, `Proc::spawn_bounded`) through [`acquire`], so at most
//! [`MAX_CONCURRENT_SUBPROCESSES`] children hold permits at a time and the
//! rest queue inside the spawn call itself.
//!
//! Long-lived children — provider sessions, services, terminals — must not
//! hold a permit: they would park it for hours and starve everything else.
//! Those spawns use `Proc::spawn`, which never gates.
//!
//! Per-label counters feed `daemon-stats.jsonl` so a saturated gate is
//! diagnosable after the fact: `wait_ms` shows the cap itself hurting,
//! `hold_ms`/`max_hold_ms` show which label hogs permits.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use waku_protocol::SubprocessLabelSample;

/// Concurrent bounded-duration children. Worker threads spend the wait
/// blocked in `recv_timeout`, not running — the cap prices the children,
/// not the threads.
const MAX_CONCURRENT_SUBPROCESSES: usize = 8;
/// A spawn that cannot get a permit this long proceeds anyway — a wedged
/// child must not freeze every future spawn forever — and logs, since the
/// gate is meant to fail only when something is already wrong.
const PERMIT_WAIT: Duration = Duration::from_secs(60);
/// One child holding a permit this long earns a stderr line; the aggregate
/// counters record that it happened but not *when*.
const OUTLIER_HOLD: Duration = Duration::from_secs(60);

type PermitTokens = (
    crossbeam_channel::Sender<()>,
    crossbeam_channel::Receiver<()>,
);

/// `Permit`-returned tokens ride a bounded channel pre-filled with
/// [`MAX_CONCURRENT_SUBPROCESSES`] units: `recv` acquires, `send` releases.
/// Only permit holders ever send, so a release always has room.
fn permits() -> &'static PermitTokens {
    static PERMITS: OnceLock<PermitTokens> = OnceLock::new();
    PERMITS.get_or_init(|| {
        let (sender, receiver) = crossbeam_channel::bounded(MAX_CONCURRENT_SUBPROCESSES);
        for _ in 0..MAX_CONCURRENT_SUBPROCESSES {
            sender.try_send(()).expect("a fresh channel has room");
        }
        (sender, receiver)
    })
}

#[derive(Default)]
struct LabelStats {
    spawns: u64,
    in_flight: u32,
    hold_ms: u64,
    max_hold_ms: u64,
    wait_ms: u64,
    max_wait_ms: u64,
}

fn stats() -> &'static Mutex<HashMap<String, LabelStats>> {
    static STATS: OnceLock<Mutex<HashMap<String, LabelStats>>> = OnceLock::new();
    STATS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The wire snapshot `crate::stats` folds into each `daemon-stats.jsonl`
/// sample. Counters accumulate over the daemon's lifetime; `in_flight`
/// reads live.
pub fn snapshot() -> BTreeMap<String, SubprocessLabelSample> {
    stats()
        .lock()
        .expect("subprocess stats lock")
        .iter()
        .map(|(label, stats)| {
            (
                label.clone(),
                SubprocessLabelSample {
                    spawns: stats.spawns,
                    in_flight: stats.in_flight,
                    hold_ms: stats.hold_ms,
                    max_hold_ms: stats.max_hold_ms,
                    wait_ms: stats.wait_ms,
                    max_wait_ms: stats.max_wait_ms,
                },
            )
        })
        .collect()
}

/// A held (or timed-out) spawn slot. Dropping returns the token and
/// records the hold; carrying it inside a child handle extends the hold to
/// the handle's lifetime.
pub struct Permit {
    tokens: crossbeam_channel::Sender<()>,
    label: String,
    /// `false` when the wait timed out and the spawn proceeds uncounted by
    /// the cap — the stats still record the wait and the hold.
    held: bool,
    started: Instant,
}

impl Drop for Permit {
    fn drop(&mut self) {
        let held_ms = self.started.elapsed().as_millis() as u64;
        {
            let mut stats = stats().lock().expect("subprocess stats lock");
            let entry = stats.entry(self.label.clone()).or_default();
            entry.in_flight = entry.in_flight.saturating_sub(1);
            entry.spawns += 1;
            entry.hold_ms += held_ms;
            entry.max_hold_ms = entry.max_hold_ms.max(held_ms);
        }
        if self.held {
            // Try-send rather than send: a full channel would mean a bug
            // minted extra tokens, and blocking inside a drop must never
            // happen.
            let _ = self.tokens.try_send(());
        }
        if held_ms >= OUTLIER_HOLD.as_millis() as u64 {
            eprintln!(
                "goddard-daemon: subprocess `{}` held a spawn slot for {held_ms}ms",
                self.label
            );
        }
    }
}

fn acquire_on(tokens: &PermitTokens, label: String, limit: Duration) -> Permit {
    {
        let mut stats = stats().lock().expect("subprocess stats lock");
        stats.entry(label.clone()).or_default().in_flight += 1;
    }
    let waited = Instant::now();
    let held = tokens.1.recv_timeout(limit).is_ok();
    let wait_ms = waited.elapsed().as_millis() as u64;
    {
        let mut stats = stats().lock().expect("subprocess stats lock");
        let entry = stats.entry(label.clone()).or_default();
        entry.wait_ms += wait_ms;
        entry.max_wait_ms = entry.max_wait_ms.max(wait_ms);
    }
    if !held {
        eprintln!(
            "goddard-daemon: subprocess `{label}` waited {wait_ms}ms for a spawn slot; spawning unbounded"
        );
    }
    Permit {
        tokens: tokens.0.clone(),
        label,
        held,
        started: Instant::now(),
    }
}

/// Wait for a spawn slot for `label`, recording the wait. Waiting longer
/// than [`PERMIT_WAIT`] logs and returns a tokenless permit — see the
/// constant.
pub fn acquire(label: String) -> Permit {
    acquire_on(permits(), label, PERMIT_WAIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn test_tokens(count: usize) -> PermitTokens {
        let (sender, receiver) = crossbeam_channel::bounded(count);
        for _ in 0..count {
            sender.try_send(()).unwrap();
        }
        (sender, receiver)
    }

    #[test]
    fn a_permit_waits_until_a_token_frees() {
        let tokens = test_tokens(1);
        let first = acquire_on(&tokens, "test-first".to_owned(), Duration::from_secs(5));
        let (done_tx, done_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let permit = acquire_on(&tokens, "test-late".to_owned(), Duration::from_secs(5));
            let _ = done_tx.send(permit.held);
            drop(permit);
        });
        assert!(
            done_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "a second acquire should still be waiting"
        );
        drop(first);
        assert!(
            done_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("a freed token should unblock the waiter")
        );
        thread.join().unwrap();
    }

    #[test]
    fn a_permit_times_out_and_spawns_unbounded() {
        let tokens = test_tokens(1);
        let first = acquire_on(&tokens, "test-holder".to_owned(), Duration::from_secs(5));
        let late = acquire_on(&tokens, "test-late".to_owned(), Duration::from_millis(50));
        assert!(first.held);
        assert!(!late.held);
        drop(late);
        drop(first);
    }
}
