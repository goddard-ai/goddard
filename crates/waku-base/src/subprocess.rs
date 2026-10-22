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
use std::io;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use waku_protocol::SubprocessLabelSample;

/// Concurrent bounded-duration children. Worker threads spend the wait
/// blocked in `recv_timeout`, not running — the cap prices the children,
/// not the threads.
const MAX_CONCURRENT_SUBPROCESSES: usize = 8;
/// A spawn that cannot get a permit this long fails without starting a child.
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

/// A held spawn slot. Dropping returns the token and
/// records the hold; carrying it inside a child handle extends the hold to
/// the handle's lifetime.
pub struct Permit {
    tokens: crossbeam_channel::Sender<()>,
    label: String,
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
        // Try-send rather than send: a full channel would mean a bug
        // minted extra tokens, and blocking inside a drop must never happen.
        let _ = self.tokens.try_send(());
        if held_ms >= OUTLIER_HOLD.as_millis() as u64 {
            eprintln!(
                "goddard-daemon: subprocess `{}` held a spawn slot for {held_ms}ms",
                self.label
            );
        }
    }
}

fn acquire_on(tokens: &PermitTokens, label: String, deadline: Instant) -> io::Result<Permit> {
    {
        let mut stats = stats().lock().expect("subprocess stats lock");
        stats.entry(label.clone()).or_default().in_flight += 1;
    }
    let waited = Instant::now();
    let held = deadline
        .checked_duration_since(waited)
        .filter(|remaining| !remaining.is_zero())
        .is_some_and(|remaining| tokens.1.recv_timeout(remaining).is_ok());
    let expired = Instant::now() >= deadline;
    let wait_ms = waited.elapsed().as_millis() as u64;
    {
        let mut stats = stats().lock().expect("subprocess stats lock");
        let entry = stats.entry(label.clone()).or_default();
        entry.wait_ms += wait_ms;
        entry.max_wait_ms = entry.max_wait_ms.max(wait_ms);
        if !held || expired {
            entry.in_flight = entry.in_flight.saturating_sub(1);
        }
    }
    if !held || expired {
        if held {
            let _ = tokens.0.try_send(());
        }
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("subprocess `{label}` deadline expired waiting for a spawn slot"),
        ));
    }
    Ok(Permit {
        tokens: tokens.0.clone(),
        label,
        started: Instant::now(),
    })
}

/// Wait for a spawn slot for `label`, failing after [`PERMIT_WAIT`].
pub fn acquire(label: String) -> io::Result<Permit> {
    acquire_until(label, Instant::now() + PERMIT_WAIT)
}

/// Acquire before the request's absolute deadline. Queueing consumes that
/// same deadline; a timed-out request never starts a child without a permit.
pub fn acquire_until(label: String, deadline: Instant) -> io::Result<Permit> {
    acquire_on(permits(), label, deadline)
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
        let first = acquire_on(
            &tokens,
            "test-first".to_owned(),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let permit = acquire_on(
                &tokens,
                "test-late".to_owned(),
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
            let _ = done_tx.send(());
            drop(permit);
        });
        assert!(
            done_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "a second acquire should still be waiting"
        );
        drop(first);
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a freed token should unblock the waiter");
        thread.join().unwrap();
    }

    #[test]
    fn a_timed_out_waiter_does_not_receive_or_mint_a_permit() {
        let tokens = test_tokens(1);
        let first = acquire_on(
            &tokens,
            "test-holder".to_owned(),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        let label = "test-expired-waiter".to_owned();
        let error = acquire_on(
            &tokens,
            label.clone(),
            Instant::now() + Duration::from_millis(20),
        )
        .err()
        .expect("saturated admission must fail");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(snapshot()[&label].in_flight, 0);
        assert_eq!(snapshot()[&label].spawns, 0);
        assert!(tokens.1.try_recv().is_err());
        drop(first);
        assert!(tokens.1.try_recv().is_ok());
        assert!(tokens.1.try_recv().is_err());
    }

    #[test]
    fn an_expired_deadline_cannot_take_an_available_slot() {
        let tokens = test_tokens(1);
        let error = acquire_on(&tokens, "test-already-expired".to_owned(), Instant::now())
            .err()
            .expect("expired admission must fail");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(tokens.1.try_recv().is_ok());
    }
}
