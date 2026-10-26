//! Admission for optional CPU/disk-heavy work. Mandatory persistence and
//! pressure reclamation must never use this gate. All methods that acquire a
//! permit belong on a worker: host coordination performs filesystem I/O.

use std::collections::{HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use crate::host_load::{Monitor, Observation};
use crate::pressure::Level;

const HEALTHY_WINDOW: Duration = Duration::from_secs(10);
const UNKNOWN_INTERVAL: Duration = Duration::from_secs(30);
const UNIT_BUDGET: Duration = Duration::from_millis(50);
const QUEUE_CAPACITY: usize = 128;
const RETRY_INTERVAL: Duration = Duration::from_secs(2);

/// Return after one independently committed unit. A job must revalidate its
/// descriptor each time; queue membership never authorizes deleting data.
pub enum Progress {
    Complete,
    More,
}

type Work = Box<dyn FnMut(&Permit) -> io::Result<Progress> + Send>;

struct Job {
    key: String,
    unit: Unit,
    work: Work,
    retry_at: Instant,
    failures: u32,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    // Includes the running job, preventing a concurrent submit from replacing
    // it or growing the queue while its worker is outside the mutex.
    keys: HashSet<String>,
}

/// A bounded process-wide queue serviced by one dedicated background thread.
/// Capture lightweight descriptors and shared services, never hydrated payloads.
pub struct Scheduler {
    queue: Mutex<Queue>,
    wake: Condvar,
}

impl Scheduler {
    pub fn shared() -> io::Result<&'static Arc<Self>> {
        static SCHEDULER: OnceLock<Result<Arc<Scheduler>, io::Error>> = OnceLock::new();
        SCHEDULER
            .get_or_init(|| {
                let scheduler = Arc::new(Self {
                    queue: Mutex::new(Queue::default()),
                    wake: Condvar::new(),
                });
                let worker = Arc::clone(&scheduler);
                std::thread::Builder::new()
                    .name("waku-maintenance".into())
                    .spawn(move || worker.run())?;
                Ok(scheduler)
            })
            .as_ref()
            .map_err(|error| io::Error::new(error.kind(), error.to_string()))
    }

    /// `false` means an equivalent job is queued or running. Full queues are
    /// explicit errors: callers must retain their durable intent for retry.
    pub fn submit(
        &self,
        key: String,
        unit: Unit,
        work: impl FnMut(&Permit) -> io::Result<Progress> + Send + 'static,
    ) -> io::Result<bool> {
        let mut queue = self.queue.lock();
        if queue.keys.contains(&key) {
            return Ok(false);
        }
        if queue.keys.len() >= QUEUE_CAPACITY {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "maintenance queue is full",
            ));
        }
        queue.keys.insert(key.clone());
        queue.jobs.push_back(Job {
            key,
            unit,
            work: Box::new(work),
            retry_at: Instant::now(),
            failures: 0,
        });
        self.wake.notify_one();
        Ok(true)
    }

    fn run(&self) {
        lower_thread_priority();
        loop {
            let mut job = {
                let mut queue = self.queue.lock();
                let now = Instant::now();
                let next = queue.jobs.iter().position(|job| job.retry_at <= now);
                let Some(index) = next else {
                    self.wake.wait_for(&mut queue, RETRY_INTERVAL);
                    continue;
                };
                queue.jobs.remove(index).expect("queued job exists")
            };
            let result = match Gate::shared().try_acquire(job.unit) {
                Ok(Ok(permit)) => (job.work)(&permit).map(Some),
                Ok(Err(_)) => Ok(None),
                Err(error) => Err(error),
            };
            let mut queue = self.queue.lock();
            match result {
                Ok(Some(Progress::Complete)) => {
                    queue.keys.remove(&job.key);
                }
                result => {
                    let delay = if let Err(error) = result {
                        job.failures = job.failures.saturating_add(1);
                        // Log the cause, never descriptor paths or user data.
                        eprintln!(
                            "maintenance unit failed (attempt {}): {error}",
                            job.failures
                        );
                        Duration::from_secs((2u64.saturating_pow(job.failures.min(8))).min(300))
                    } else {
                        job.failures = 0;
                        RETRY_INTERVAL
                    };
                    job.retry_at = Instant::now() + delay;
                    queue.jobs.push_back(job);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    /// Cooperative, bounded work that can yield within about 50 ms.
    Bounded,
    /// An opaque operation such as Git or a large SQLite transaction. Unknown
    /// telemetry never authorizes starting one of these.
    Heavy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitReason {
    HealthyWindow,
    CpuBusy,
    MemoryPressure,
    Unknown,
    HostBusy,
}

#[derive(Default)]
struct Policy {
    healthy_since: Option<Instant>,
    unknown_since: Option<Instant>,
    last_fallback: Option<Instant>,
}

impl Policy {
    fn eligibility(
        &mut self,
        sample: Observation,
        now: Instant,
        unit: Unit,
    ) -> Result<bool, WaitReason> {
        // A stale critical/elevated observation does not become permission to
        // allocate more memory. Wait for an explicitly healthy observation.
        if matches!(sample.memory, Some(Level::Elevated | Level::Critical)) {
            self.healthy_since = None;
            return Err(WaitReason::MemoryPressure);
        }
        let known = sample.fresh(now) && sample.cpu_percent.is_some() && sample.memory.is_some();
        if !known {
            self.healthy_since = None;
            let since = *self.unknown_since.get_or_insert(now);
            let retry = self.last_fallback.unwrap_or(since);
            return if unit == Unit::Bounded
                && now.saturating_duration_since(retry) >= UNKNOWN_INTERVAL
            {
                Ok(true)
            } else {
                Err(WaitReason::Unknown)
            };
        }
        self.unknown_since = None;
        if sample.cpu_percent.is_none_or(|cpu| cpu >= 40.0) {
            self.healthy_since = None;
            return Err(WaitReason::CpuBusy);
        }
        let since = *self.healthy_since.get_or_insert(now);
        let since = since.max(sample.healthy_since.unwrap_or(since));
        self.healthy_since = Some(since);
        if now.saturating_duration_since(since) < HEALTHY_WINDOW {
            Err(WaitReason::HealthyWindow)
        } else {
            Ok(false)
        }
    }
}

pub struct Gate {
    monitor: Arc<Monitor>,
    policy: Mutex<Policy>,
}

pub struct Permit {
    _host: File,
    monitor: Arc<Monitor>,
    started: Instant,
    fallback: bool,
    unit: Unit,
}

impl Gate {
    pub fn shared() -> &'static Self {
        static GATE: OnceLock<Gate> = OnceLock::new();
        GATE.get_or_init(|| Self {
            monitor: Monitor::shared(),
            policy: Mutex::new(Policy::default()),
        })
    }

    /// Nonblocking admission. A denied job should retain only its descriptor
    /// and retry on a timer; it must not hydrate payloads while waiting.
    pub fn try_acquire(&self, unit: Unit) -> io::Result<Result<Permit, WaitReason>> {
        let now = Instant::now();
        let mut fallback = match self
            .policy
            .lock()
            .eligibility(self.monitor.snapshot(), now, unit)
        {
            Ok(fallback) => fallback,
            Err(reason) => return Ok(Err(reason)),
        };
        let root = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("org.goddardai.maintenance");
        std::fs::create_dir_all(&root)?;
        let Some(host) = try_host_lock(&root.join("host.lock"))? else {
            return Ok(Err(WaitReason::HostBusy));
        };
        if fallback {
            let mut policy = self.policy.lock();
            // Another worker may have consumed the fallback while this one
            // was obtaining the host slot. Recheck before spending it.
            fallback = match policy.eligibility(self.monitor.snapshot(), Instant::now(), unit) {
                Ok(fallback) => fallback,
                Err(reason) => return Ok(Err(reason)),
            };
            if fallback {
                policy.last_fallback = Some(now);
            }
        }
        Ok(Ok(Permit {
            _host: host,
            monitor: Arc::clone(&self.monitor),
            started: now,
            fallback,
            unit,
        }))
    }
}

impl Permit {
    /// Recheck between cooperative units. This cannot preempt a running Git
    /// process or transaction; those must stay small and use background QoS.
    pub fn should_continue(&self) -> bool {
        let now = Instant::now();
        if self.unit == Unit::Bounded && now.saturating_duration_since(self.started) >= UNIT_BUDGET
        {
            return false;
        }
        let sample = self.monitor.snapshot();
        if matches!(sample.memory, Some(Level::Elevated | Level::Critical)) {
            return false;
        }
        if sample.fresh(now) && sample.cpu_percent.is_some_and(|cpu| cpu >= 60.0) {
            return false;
        }
        if self.fallback {
            return now.saturating_duration_since(self.started) < UNIT_BUDGET;
        }
        sample.fresh(now)
            && sample.memory == Some(Level::Normal)
            && sample.cpu_percent.is_some_and(|cpu| cpu < 60.0)
    }
}

#[cfg(unix)]
fn try_host_lock(path: &Path) -> io::Result<Option<File>> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    // SAFETY: file owns a live descriptor. Nonblocking flock does not access
    // memory and the lock lives exactly as long as this open file description.
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if locked == 0 {
        return Ok(Some(file));
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        Ok(None)
    } else {
        Err(error)
    }
}

#[cfg(not(unix))]
fn try_host_lock(_path: &Path) -> io::Result<Option<File>> {
    // Unknown host coordination is not permission to run unbounded jobs.
    Ok(None)
}

/// Apply only to dedicated maintenance threads, never shared request workers.
pub fn lower_thread_priority() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(class: u32, relative: i32) -> i32;
        }
        // SAFETY: valid QOS_CLASS_BACKGROUND and relative priority zero affect
        // only the calling thread. Failure leaves ordinary scheduling intact.
        let _ = unsafe { pthread_set_qos_class_self_np(0x09, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_deduplicates_and_applies_backpressure_before_accepting_work() {
        let scheduler = Scheduler {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
        };
        for index in 0..QUEUE_CAPACITY {
            assert!(
                scheduler
                    .submit(index.to_string(), Unit::Heavy, |_| Ok(Progress::Complete))
                    .unwrap()
            );
        }
        assert!(
            !scheduler
                .submit("0".into(), Unit::Heavy, |_| Ok(Progress::Complete))
                .unwrap()
        );
        assert_eq!(
            scheduler
                .submit("overflow".into(), Unit::Heavy, |_| Ok(Progress::Complete))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
    }

    fn observation(now: Instant, cpu: Option<f32>, memory: Option<Level>) -> Observation {
        Observation {
            cpu_percent: cpu,
            memory,
            observed_at: now,
            healthy_since: None,
        }
    }

    #[test]
    fn a_busy_gap_between_admissions_requires_a_new_sampled_healthy_window() {
        let start = Instant::now();
        let mut policy = Policy::default();
        let _ = policy.eligibility(
            observation(start, Some(10.0), Some(Level::Normal)),
            start,
            Unit::Heavy,
        );
        let now = start + Duration::from_secs(100);
        let since = now - Duration::from_secs(3);
        let mut sample = observation(now, Some(10.0), Some(Level::Normal));
        sample.healthy_since = Some(since);
        assert_eq!(
            policy.eligibility(sample, now, Unit::Heavy),
            Err(WaitReason::HealthyWindow)
        );
        let now = now + Duration::from_secs(7);
        sample.observed_at = now;
        assert_eq!(policy.eligibility(sample, now, Unit::Heavy), Ok(false));
    }

    #[test]
    fn busy_or_pressured_samples_restart_the_healthy_window() {
        let start = Instant::now();
        let mut policy = Policy::default();
        assert_eq!(
            policy.eligibility(
                observation(start, Some(20.0), Some(Level::Normal)),
                start,
                Unit::Heavy
            ),
            Err(WaitReason::HealthyWindow)
        );
        let now = start + HEALTHY_WINDOW;
        assert_eq!(
            policy.eligibility(
                observation(now, Some(20.0), Some(Level::Normal)),
                now,
                Unit::Heavy
            ),
            Ok(false)
        );
        assert_eq!(
            policy.eligibility(
                observation(now, Some(70.0), Some(Level::Normal)),
                now,
                Unit::Heavy
            ),
            Err(WaitReason::CpuBusy)
        );
        assert_eq!(
            policy.eligibility(
                observation(now, Some(20.0), Some(Level::Normal)),
                now,
                Unit::Heavy
            ),
            Err(WaitReason::HealthyWindow)
        );
        assert_eq!(
            policy.eligibility(
                observation(now, Some(0.0), Some(Level::Critical)),
                now,
                Unit::Bounded
            ),
            Err(WaitReason::MemoryPressure)
        );
    }

    #[test]
    fn unknown_fallback_is_bounded_and_never_overrides_memory_pressure() {
        let start = Instant::now();
        let mut policy = Policy::default();
        let unknown = observation(start, None, None);
        assert_eq!(
            policy.eligibility(unknown, start, Unit::Bounded),
            Err(WaitReason::Unknown)
        );
        let now = start + UNKNOWN_INTERVAL;
        assert_eq!(
            policy.eligibility(unknown, now, Unit::Heavy),
            Err(WaitReason::Unknown)
        );
        assert_eq!(policy.eligibility(unknown, now, Unit::Bounded), Ok(true));
        policy.last_fallback = Some(now);
        assert_eq!(
            policy.eligibility(unknown, now, Unit::Bounded),
            Err(WaitReason::Unknown)
        );
        assert_eq!(
            policy.eligibility(
                observation(start, None, Some(Level::Critical)),
                now,
                Unit::Bounded
            ),
            Err(WaitReason::MemoryPressure)
        );
    }

    #[cfg(unix)]
    #[test]
    fn independent_workers_cannot_hold_the_same_host_slot() {
        let path =
            std::env::temp_dir().join(format!("goddard-maintenance-{}.lock", uuid::Uuid::new_v4()));
        let first = try_host_lock(&path).unwrap().unwrap();
        assert!(try_host_lock(&path).unwrap().is_none());
        drop(first);
        let next = try_host_lock(&path).unwrap().unwrap();
        drop(next);
        std::fs::remove_file(path).unwrap();
    }
}
