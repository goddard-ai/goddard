//! Host-wide observations for discretionary maintenance. Sampling performs
//! native I/O on one background thread; consumers read only a cached snapshot.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::pressure::{self, Level};

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const STALE_AFTER: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug)]
pub struct Observation {
    /// Percentage of total host CPU capacity, rather than percent of one core.
    pub cpu_percent: Option<f32>,
    pub memory: Option<Level>,
    pub observed_at: Instant,
    /// Start of uninterrupted healthy samples, reset by busy, pressured, or
    /// unavailable telemetry even when no maintenance job is asking to run.
    pub healthy_since: Option<Instant>,
}

impl Observation {
    pub fn fresh(self, now: Instant) -> bool {
        now.saturating_duration_since(self.observed_at) <= STALE_AFTER
    }
}

pub struct Monitor {
    latest: Mutex<Observation>,
}

impl Monitor {
    /// Starts at most one native sampler per process. Call from initialization
    /// or a worker; rendering must never initialize a background service.
    pub fn shared() -> Arc<Self> {
        static MONITOR: OnceLock<Arc<Monitor>> = OnceLock::new();
        Arc::clone(MONITOR.get_or_init(|| {
            let monitor = Arc::new(Self {
                latest: Mutex::new(Observation {
                    cpu_percent: None,
                    memory: None,
                    observed_at: Instant::now(),
                    healthy_since: None,
                }),
            });
            let weak = Arc::downgrade(&monitor);
            if let Err(error) = std::thread::Builder::new()
                .name("waku-host-load".into())
                .spawn(move || {
                    let mut previous = cpu_ticks();
                    loop {
                        std::thread::sleep(SAMPLE_INTERVAL);
                        let Some(monitor) = weak.upgrade() else { break };
                        let next = cpu_ticks();
                        let cpu_percent = previous.zip(next).and_then(|(a, b)| utilization(a, b));
                        previous = next;
                        let memory = pressure::sample();
                        let observed_at = Instant::now();
                        let mut latest = monitor.latest.lock();
                        let healthy_since = if cpu_percent.is_some_and(|cpu| cpu < 40.0)
                            && memory == Some(Level::Normal)
                        {
                            latest.healthy_since.or(Some(observed_at))
                        } else {
                            None
                        };
                        *latest = Observation {
                            cpu_percent,
                            memory,
                            observed_at,
                            healthy_since,
                        };
                    }
                })
            {
                // Unknown remains unknown when a sampler cannot start.
                eprintln!("could not start host-load sampler: {error}");
            }
            monitor
        }))
    }

    pub fn snapshot(&self) -> Observation {
        *self.latest.lock()
    }
}

#[derive(Clone, Copy)]
struct CpuTicks {
    total: u64,
    idle: u64,
}

fn utilization(previous: CpuTicks, next: CpuTicks) -> Option<f32> {
    // Counter resets/wraps and a zero observation interval are unavailable
    // observations, never evidence that the host became idle.
    let elapsed = next.total.checked_sub(previous.total)?;
    let idle = next.idle.checked_sub(previous.idle)?;
    if elapsed == 0 || idle > elapsed {
        return None;
    }
    Some(100.0 * (elapsed - idle) as f32 / elapsed as f32)
}

#[cfg(target_os = "macos")]
fn cpu_ticks() -> Option<CpuTicks> {
    unsafe extern "C" {
        fn mach_host_self() -> u32;
        fn host_statistics(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
    }
    // Keep a single send right for the process lifetime instead of leaking one
    // per sample. HOST_CPU_LOAD_INFO is four 32-bit counters: user/system/idle/nice.
    static HOST: OnceLock<u32> = OnceLock::new();
    // SAFETY: mach_host_self takes no arguments and returns a host send right.
    let host = *HOST.get_or_init(|| unsafe { mach_host_self() });
    let mut ticks = [0u32; 4];
    let mut count = 4u32;
    // SAFETY: HOST_CPU_LOAD_INFO (3) writes exactly four integer counters into
    // the initialized, correctly aligned output buffer. Count is in integers.
    let result = unsafe { host_statistics(host, 3, ticks.as_mut_ptr().cast(), &mut count) };
    (result == 0 && count == 4).then(|| CpuTicks {
        total: ticks.iter().map(|value| u64::from(*value)).sum(),
        idle: u64::from(ticks[2]),
    })
}

#[cfg(target_os = "linux")]
fn cpu_ticks() -> Option<CpuTicks> {
    let contents = std::fs::read_to_string("/proc/stat").ok()?;
    linux_cpu_ticks(contents.lines().next()?)
}

#[cfg(any(target_os = "linux", test))]
fn linux_cpu_ticks(line: &str) -> Option<CpuTicks> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    // Guest time is already included in user/nice. Sum only the first eight
    // counters, keeping iowait idle to match OS CPU-utilization conventions.
    let ticks = fields
        .take(8)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if ticks.len() < 4 {
        return None;
    }
    Some(CpuTicks {
        total: ticks
            .iter()
            .try_fold(0u64, |sum, value| sum.checked_add(*value))?,
        idle: ticks[3].checked_add(ticks.get(4).copied().unwrap_or(0))?,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn cpu_ticks() -> Option<CpuTicks> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utilization_uses_host_capacity_and_rejects_counter_resets() {
        let first = CpuTicks {
            total: 1000,
            idle: 600,
        };
        let next = CpuTicks {
            total: 1800,
            idle: 1200,
        };
        assert_eq!(utilization(first, next), Some(25.0));
        assert_eq!(utilization(first, first), None);
        assert_eq!(utilization(next, first), None);
    }

    #[test]
    fn linux_cpu_sample_does_not_double_count_guest_ticks() {
        let ticks = linux_cpu_ticks("cpu 100 20 30 400 10 2 3 5 50 10").unwrap();
        assert_eq!(ticks.total, 570);
        assert_eq!(ticks.idle, 410);
        assert!(linux_cpu_ticks("cpu0 1 2 3 4").is_none());
        assert!(linux_cpu_ticks("cpu 1 invalid 3 4").is_none());
    }
}
