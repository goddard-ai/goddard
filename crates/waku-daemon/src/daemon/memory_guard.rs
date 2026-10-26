use super::*;
use crate::pressure::Level;
use std::time::{Duration, Instant};

mod telemetry;
use telemetry::{Eviction, IdleEviction, Outcome, Reason};

const INTERVAL: Duration = Duration::from_secs(5);
const OVERAGE: Duration = Duration::from_secs(30);
const CANCEL_GRACE: Duration = Duration::from_secs(10);
const TERM_GRACE: Duration = Duration::from_secs(2);
const DEFAULT_LIMIT_MB: u64 = 4096;

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
struct Recovery {
    paused: bool,
    restarted: bool,
}

/// Durable pause and retry fences outlive daemon replacement. Process ownership
/// is deliberately ephemeral and must be proved again by each new launch.
pub(super) struct MemoryGuard {
    directory: PathBuf,
    anchors: PathBuf,
    recovery: Mutex<HashMap<Uuid, Recovery>>,
    stopping: Mutex<HashSet<Uuid>>,
    critical: std::sync::atomic::AtomicBool,
    eviction_writer: Mutex<()>,
    idle_evictions: Mutex<Vec<IdleEviction>>,
}

impl MemoryGuard {
    pub(super) fn open(data: &Path) -> anyhow::Result<Self> {
        let directory = data.join("memory-guard");
        let anchors = directory.join(Uuid::new_v4().to_string());
        waku_base::fs_ext::create_private_dir_all(&anchors)?;
        waku_base::fs_ext::restrict_to_owner(&directory)?;
        let recovery = match std::fs::read(directory.join("recovery.json")) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).context("invalid memory recovery ledger")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            directory,
            anchors,
            recovery: Mutex::new(recovery),
            stopping: Mutex::new(HashSet::new()),
            critical: false.into(),
            eviction_writer: Mutex::new(()),
            idle_evictions: Mutex::new(Vec::new()),
        })
    }

    fn save(&self, recovery: &HashMap<Uuid, Recovery>) -> anyhow::Result<()> {
        use std::io::Write as _;
        let temporary = self.directory.join(format!(".recovery-{}", Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = options.open(&temporary)?;
            file.write_all(&serde_json::to_vec(recovery)?)?;
            file.sync_all()?;
            std::fs::rename(&temporary, self.directory.join("recovery.json"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }

    pub(super) fn anchor(&self, runtime: Uuid) -> PathBuf {
        self.anchors.join(runtime.to_string())
    }

    pub(super) fn host_critical(&self) -> bool {
        self.critical.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(super) fn paused(&self, session: Uuid) -> bool {
        self.recovery.lock().get(&session).is_some_and(|r| r.paused)
    }

    pub(super) fn check_admission(&self, session: Uuid) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.host_critical(),
            "new work is held while host memory pressure is critical"
        );
        anyhow::ensure!(
            !self.paused(session),
            "task paused by the memory guard; continue it explicitly after reviewing its interrupted work"
        );
        Ok(())
    }

    pub(super) fn user_resume(&self, session: Uuid) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.stopping.lock().contains(&session),
            "task memory containment is still stopping its processes"
        );
        let mut recovery = self.recovery.lock();
        if let Some(entry) = recovery.get_mut(&session) {
            let previous = *entry;
            entry.paused = false;
            if let Err(error) = self.save(&recovery) {
                recovery.insert(session, previous);
                return Err(error);
            }
        }
        Ok(())
    }

    fn pause(&self, session: Uuid) -> anyhow::Result<()> {
        self.stopping.lock().insert(session);
        let mut recovery = self.recovery.lock();
        recovery.entry(session).or_default().paused = true;
        self.save(&recovery)
    }

    fn claim_restart(&self, session: Uuid) -> anyhow::Result<bool> {
        let mut recovery = self.recovery.lock();
        let entry = recovery.entry(session).or_default();
        if entry.restarted {
            return Ok(false);
        }
        let previous = *entry;
        entry.restarted = true;
        entry.paused = false;
        if let Err(error) = self.save(&recovery) {
            recovery.insert(session, previous);
            return Err(error);
        }
        Ok(true)
    }

    fn record_incident(
        &self,
        session: Uuid,
        runtime: Uuid,
        bytes: u64,
        limit_mb: u64,
        critical: bool,
        owned: &HashSet<Process>,
    ) {
        let incident = serde_json::json!({
            "at": crate::model::unix_time(), "task": session, "runtime": runtime,
            "bytes": bytes, "limitMb": limit_mb, "critical": critical,
            "processes": owned.iter().map(|p| p.pid).take(64).collect::<Vec<_>>()
        });
        if let Ok(mut line) = serde_json::to_vec(&incident) {
            line.push(b'\n');
            if let Err(error) = waku_base::fs_ext::append_capped(
                &self.directory.join("incidents.jsonl"),
                &line,
                256 * 1024,
            ) {
                eprintln!("memory guard could not write incident: {error}");
            }
        }
    }
}

impl Drop for MemoryGuard {
    fn drop(&mut self) {
        // These are only this daemon incarnation's ephemeral registration files.
        let _ = std::fs::remove_dir_all(&self.anchors);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Process {
    pid: i32,
    birth: u64,
}

impl Process {
    fn current(self, table: &HashMap<i32, crate::stats::ProcEntry>) -> bool {
        table
            .get(&self.pid)
            .is_some_and(|entry| entry.birth == self.birth)
    }

    #[cfg(unix)]
    fn signal(self, signal: i32) -> bool {
        if self.pid > 1 && waku_base::process_identity::start_time(self.pid) == Some(self.birth) {
            // SAFETY: pid is positive, identifies a captured owned process, and its
            // birth stamp was rechecked immediately before this signal.
            unsafe {
                return libc::kill(self.pid, signal) == 0;
            }
        }
        false
    }
}

fn registered_process(
    path: &Path,
    table: &HashMap<i32, crate::stats::ProcEntry>,
) -> Option<Process> {
    let registration = std::fs::read_to_string(path).ok()?;
    let mut fields = registration.split_whitespace();
    let pid: i32 = fields.next()?.parse().ok()?;
    let entry = table.get(&pid)?;
    if entry.ppid != i32::try_from(std::process::id()).ok()? {
        return None;
    }
    if let Some(birth) = fields.next() {
        if birth.parse::<u64>().ok()? != entry.birth {
            return None;
        }
    } else if !waku_base::process_identity::has_argument(
        pid,
        &format!("goddard-runtime:{}", path.display()),
    ) {
        // The ACP SDK launches asynchronously. Its guardian registers its PID;
        // the unique argv marker proves which launch owns that PID on first read.
        return None;
    }
    Some(Process {
        pid,
        birth: entry.birth,
    })
}

fn refresh_tree(
    owned: &mut HashSet<Process>,
    table: &HashMap<i32, crate::stats::ProcEntry>,
) -> u64 {
    owned.retain(|process| process.current(table));
    let mut descendants: Vec<_> = owned.iter().map(|process| process.pid).collect();
    let mut children: HashMap<i32, Vec<Process>> = HashMap::new();
    for (&pid, entry) in table {
        children.entry(entry.ppid).or_default().push(Process {
            pid,
            birth: entry.birth,
        });
    }
    let mut cursor = 0;
    while let Some(pid) = descendants.get(cursor).copied() {
        cursor += 1;
        for child in children.get(&pid).into_iter().flatten() {
            if owned.insert(*child) {
                descendants.push(child.pid);
            }
        }
    }
    owned
        .iter()
        .filter_map(|p| table.get(&p.pid))
        .fold(0_u64, |sum, p| sum.saturating_add(p.rss))
}

#[derive(Default)]
struct Sustained {
    since: Option<Instant>,
    last: Option<Instant>,
}

impl Sustained {
    fn observe(&mut self, now: Instant, above: bool) -> bool {
        if !above
            || self
                .last
                .is_some_and(|last| now.saturating_duration_since(last) > INTERVAL * 2)
        {
            self.since = None;
        }
        self.last = Some(now);
        if !above {
            return false;
        }
        now.saturating_duration_since(*self.since.get_or_insert(now)) >= OVERAGE
    }
}

enum Phase {
    Watching,
    Cancelling { at: Instant },
    Terminating { at: Instant },
    Stopped,
}

struct Watch {
    session: Uuid,
    root: Process,
    owned: HashSet<Process>,
    bytes: u64,
    sustained: Sustained,
    phase: Phase,
    safe_restart: bool,
    paused_task: bool,
    incident: Option<Eviction>,
    outcome: Outcome,
}

#[cfg(unix)]
impl Watch {
    fn signal(&mut self, signal: i32, outcome: Outcome, guard: &MemoryGuard, now: Instant) {
        let mut sent = false;
        for process in &self.owned {
            sent |= process.signal(signal);
        }
        if sent && self.outcome != outcome {
            self.outcome = outcome;
            if let Some(incident) = &self.incident {
                guard.record_eviction(incident, "escalated", Some(outcome), now);
            }
        }
    }
}

impl WakuBackend {
    pub(super) fn start_memory_guard(&self) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let backend = self.weak_self();
            let _ = std::thread::Builder::new()
                .name("waku-memory-guard".into())
                .spawn(move || {
                    let mut watches = HashMap::new();
                    loop {
                        let Some(backend) = backend.upgrade() else {
                            break;
                        };
                        if backend
                            .runtime_lifecycles
                            .shutting_down
                            .load(std::sync::atomic::Ordering::Acquire)
                        {
                            break;
                        }
                        backend.memory_guard_tick(&mut watches, Instant::now());
                        drop(backend);
                        std::thread::sleep(INTERVAL);
                    }
                });
        }
    }

    #[cfg(unix)]
    fn memory_guard_tick(&self, watches: &mut HashMap<Uuid, Watch>, now: Instant) {
        let observation = waku_base::host_load::Monitor::shared().snapshot();
        // Preserve immediate emergency sampling during monitor warmup or a
        // sampler failure; normal ticks reuse the cheap shared observation.
        let pressure = if observation.fresh(now) && observation.memory.is_some() {
            observation.memory
        } else {
            crate::pressure::sample()
        };
        let critical = pressure == Some(Level::Critical);
        self.memory_guard
            .critical
            .store(critical, std::sync::atomic::Ordering::Release);
        let table = crate::stats::process_table();
        self.memory_guard
            .observe_idle_evictions(now, table.as_ref());
        let Some(table) = table else {
            return;
        };
        self.memory_guard_sample(watches, now, &table, pressure);
    }

    #[cfg(unix)]
    fn memory_guard_sample(
        &self,
        watches: &mut HashMap<Uuid, Watch>,
        now: Instant,
        table: &HashMap<i32, crate::stats::ProcEntry>,
        pressure: Option<Level>,
    ) {
        let critical = pressure == Some(Level::Critical);
        let pressured = matches!(pressure, Some(Level::Elevated | Level::Critical));
        self.memory_guard
            .critical
            .store(critical, std::sync::atomic::Ordering::Release);
        let live: Vec<_> = self
            .sessions
            .lock()
            .iter()
            .filter(|(_, entry)| entry.stable.is_none())
            .map(|(&session, entry)| (session, entry.runtime_id))
            .collect();
        for (session, runtime) in live {
            if watches.contains_key(&runtime) {
                continue;
            }
            if let Some(root) = registered_process(&self.memory_guard.anchor(runtime), &table) {
                watches.insert(
                    runtime,
                    Watch {
                        session,
                        root,
                        owned: HashSet::from([root]),
                        bytes: 0,
                        sustained: Sustained::default(),
                        phase: Phase::Watching,
                        safe_restart: false,
                        paused_task: false,
                        incident: None,
                        outcome: Outcome::Cancelled,
                    },
                );
            }
        }
        let settings = self.settings.get();
        let limit_mb = settings.agent_memory_limit_mb.unwrap_or(DEFAULT_LIMIT_MB);
        let limit = limit_mb.saturating_mul(1 << 20);
        // Only an explicit budget is hard while the host is healthy (or its
        // pressure signal is unavailable). Default overage needs real pressure.
        let enforce_budget = limit != 0 && (settings.agent_memory_limit_mb.is_some() || pressured);
        let mut over_budget = HashSet::new();
        for (&runtime, watch) in watches.iter_mut() {
            watch.bytes = refresh_tree(&mut watch.owned, &table);
            if watch
                .sustained
                .observe(now, enforce_budget && watch.bytes > limit)
            {
                over_budget.insert(runtime);
            }
        }
        let idle: HashMap<_, _> = if settings.runtime_idle_timeout_secs != Some(0) {
            let state = self.task_state.lock();
            let sessions = self.sessions.lock();
            sessions
                .iter()
                .filter(|(session, entry)| runtime_evictable(&state, **session, entry, &self.agent))
                .map(|(&session, entry)| (entry.runtime_id, session))
                .collect()
        } else {
            HashMap::new()
        };
        // Every intervention waits for the preceding tree to exit. Even an
        // exit observed this tick gets a fresh host sample before another victim.
        let containing = watches.values().any(|w| {
            matches!(
                w.phase,
                Phase::Cancelling { .. } | Phase::Terminating { .. }
            )
        });
        let victim = (!containing)
            .then(|| {
                let watching = || {
                    watches
                        .iter()
                        .filter(|(_, w)| matches!(w.phase, Phase::Watching) && !w.owned.is_empty())
                };
                if pressured {
                    if let Some((&runtime, _)) = idle
                        .iter()
                        .filter(|(runtime, _)| {
                            watches
                                .get(*runtime)
                                .is_none_or(|w| matches!(w.phase, Phase::Watching))
                        })
                        .max_by_key(|(runtime, _)| watches.get(*runtime).map_or(0, |w| w.bytes))
                    {
                        return Some((runtime, Reason::IdlePressure));
                    }
                }
                let emergency = if critical && limit != 0 {
                    watching()
                        .filter(|(_, w)| w.bytes >= 256 * (1 << 20))
                        .max_by_key(|(_, w)| w.bytes)
                        .map(|(&runtime, _)| (runtime, Reason::CriticalPressure))
                } else {
                    None
                };
                emergency.or_else(|| {
                    watching()
                        .filter(|(runtime, _)| over_budget.contains(*runtime))
                        .max_by_key(|(runtime, w)| (idle.contains_key(*runtime), w.bytes))
                        .map(|(&runtime, _)| (runtime, Reason::MemoryLimit))
                })
            })
            .flatten();
        // Stable links and drivers without a registered local process tree
        // still get idle reclamation. Their process exit remains unobserved;
        // never invent identities or signal an unowned tree.
        if let Some((runtime, Reason::IdlePressure)) = victim
            && !watches.contains_key(&runtime)
            && let Some(&session) = idle.get(&runtime)
            && let Some(driver) = self.take_idle_runtime(session, runtime)
        {
            self.memory_guard
                .track_idle_eviction(session, runtime, true, Some(table));
            evict_idle_runtime(session, runtime, driver, &self.event_source());
        }
        let mut retire = Vec::new();
        for (&runtime, watch) in watches.iter_mut() {
            match watch.phase {
                Phase::Watching => {
                    if watch.owned.is_empty() {
                        retire.push(runtime);
                        continue;
                    }
                    let Some((_, reason)) = victim.filter(|(id, _)| *id == runtime) else {
                        continue;
                    };
                    let incident = Eviction::new(
                        watch.session,
                        runtime,
                        reason,
                        Some(watch.bytes),
                        Some(limit_mb),
                        &watch.owned,
                        now,
                    );
                    if idle.contains_key(&runtime) {
                        let Some(driver) = self.take_idle_runtime(watch.session, runtime) else {
                            continue;
                        };
                        self.memory_guard
                            .record_eviction(&incident, "started", None, now);
                        watch.incident = Some(incident);
                        watch.outcome = Outcome::ExitedAfterShutdown;
                        evict_idle_runtime(watch.session, runtime, driver, &self.event_source());
                        watch.phase = Phase::Cancelling {
                            at: if critical { now - CANCEL_GRACE } else { now },
                        };
                        continue;
                    }
                    let lifecycle = self.runtime_lifecycles.session(watch.session);
                    let _owner = lifecycle.lock();
                    self.memory_guard
                        .record_eviction(&incident, "started", None, now);
                    watch.incident = Some(incident);
                    let entry = self
                        .sessions
                        .lock()
                        .get(&watch.session)
                        .filter(|e| e.runtime_id == runtime)
                        .map(|entry| (entry.driver.clone(), entry.resumable));
                    let Some((driver, resumable)) = entry else {
                        // A removed/replaced runtime can leave heavy descendants.
                        // Its captured identities still authorize cleanup, but
                        // never authorize pausing the replacement task runtime.
                        watch.signal(libc::SIGTERM, Outcome::Terminated, &self.memory_guard, now);
                        watch.phase = Phase::Terminating { at: now };
                        continue;
                    };
                    // Reopening an already-settled provider is safe. An active
                    // tool's outcome after cancellation is not evidence of safe replay.
                    watch.safe_restart = resumable
                        && !self.boss.is_managed(watch.session)
                        && !self.agent.has_open_turn(watch.session)
                        && !self.agent.has_queued(watch.session)
                        && self
                            .task_state
                            .lock()
                            .sessions
                            .iter()
                            .find(|s| s.id == watch.session)
                            .is_some_and(|s| {
                                !s.status.is_busy()
                                    && s.active_turn_id().is_none()
                                    && s.queued_messages.is_empty()
                                    && s.provider_cursor.is_some()
                            });
                    if let Err(error) = self.memory_guard.pause(watch.session) {
                        eprintln!(
                            "memory guard could not persist pause for {}: {error:#}",
                            watch.session
                        );
                        watch.safe_restart = false;
                    }
                    watch.paused_task = true;
                    self.agent.note_user_stop(watch.session);
                    driver.cancel();
                    let reason = if matches!(reason, Reason::CriticalPressure) {
                        "critical host memory pressure"
                    } else {
                        "a sustained memory limit breach"
                    };
                    let notice = format!(
                        "Task paused after {reason}: its process tree used {} MiB (budget {limit_mb} MiB). Running tools are being stopped. Review interrupted work before continuing.",
                        watch.bytes / (1 << 20)
                    );
                    if let Err(error) = record_daemon_interrupt(
                        &self.task_state,
                        &self.task_store,
                        watch.session,
                        Some(&notice),
                    ) {
                        eprintln!("memory guard could not record interruption: {error:#}");
                    }
                    self.notify_task_state();
                    self.memory_guard.record_incident(
                        watch.session,
                        runtime,
                        watch.bytes,
                        limit_mb,
                        critical,
                        &watch.owned,
                    );
                    eprintln!(
                        "memory guard: task={} runtime={runtime} pid={} bytes={} limit_mb={limit_mb} critical={critical}",
                        watch.session, watch.root.pid, watch.bytes
                    );
                    watch.phase = Phase::Cancelling {
                        at: if critical { now - CANCEL_GRACE } else { now },
                    };
                }
                Phase::Cancelling { at }
                    if now.saturating_duration_since(at) >= CANCEL_GRACE
                        || watch.owned.is_empty() =>
                {
                    watch.signal(libc::SIGTERM, Outcome::Terminated, &self.memory_guard, now);
                    let lifecycle = self.runtime_lifecycles.session(watch.session);
                    let _owner = lifecycle.lock();
                    let removed = self.remove_runtime(watch.session, Some(runtime), false);
                    if removed.is_some() {
                        let sink = self.event_source().for_session(watch.session, runtime);
                        sink.notify_runtime_ended();
                        sink.end_session_runtime();
                    }
                    drop_detached(removed);
                    watch.phase = Phase::Terminating { at: now };
                }
                Phase::Terminating { at } => {
                    if watch.owned.is_empty() {
                        if watch.paused_task {
                            self.memory_guard.stopping.lock().remove(&watch.session);
                        }
                        if let Some(incident) = &watch.incident {
                            self.memory_guard.record_eviction(
                                incident,
                                "completed",
                                Some(watch.outcome),
                                now,
                            );
                        }
                        watch.phase = Phase::Stopped;
                    } else if now.saturating_duration_since(at) >= TERM_GRACE {
                        watch.signal(libc::SIGKILL, Outcome::ForceKilled, &self.memory_guard, now);
                    }
                }
                Phase::Stopped if !critical => {
                    if watch.safe_restart {
                        match self.memory_guard.claim_restart(watch.session) {
                            Ok(true) => {
                                if let Err(error) =
                                    self.ensure_agent_runtime(watch.session, &self.event_source())
                                {
                                    let _ = self.memory_guard.pause(watch.session);
                                    self.memory_guard.stopping.lock().remove(&watch.session);
                                    eprintln!(
                                        "memory guard could not reopen settled provider: {error:#}"
                                    );
                                }
                            }
                            Ok(false) => {}
                            Err(error) => {
                                eprintln!("memory guard could not claim recovery: {error:#}")
                            }
                        }
                    }
                    retire.push(runtime);
                }
                _ => {}
            }
        }
        for runtime in retire {
            watches.remove(&runtime);
            let _ = std::fs::remove_file(self.memory_guard.anchor(runtime));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!("memory-guard-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(unix)]
    #[derive(Default)]
    struct PolicyDriver {
        cancels: std::sync::atomic::AtomicUsize,
        shutdowns: std::sync::atomic::AtomicUsize,
    }

    #[cfg(unix)]
    impl driver::DriverControl for PolicyDriver {
        fn prompt(&self, _: String) {
            panic!("policy must not replay work");
        }
        fn cancel(&self) {
            self.cancels
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        fn begin_shutdown(&self) {
            self.shutdowns
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        fn respond(&self, _: String, _: String) {}
        fn rollback(&self, _: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
            Ok(None)
        }
    }

    #[cfg(unix)]
    struct PolicyFixture {
        root: PathBuf,
        backend: WakuBackend,
        table: HashMap<i32, crate::stats::ProcEntry>,
        watches: HashMap<Uuid, Watch>,
        start: Instant,
    }

    #[cfg(unix)]
    impl PolicyFixture {
        fn new(limit: Option<u64>) -> Self {
            let root = directory();
            let (backend, _) = super::super::tests::surface_test_backend(&root);
            let mut settings = backend.settings.get();
            settings.agent_memory_limit_mb = limit;
            backend.settings.replace(settings).unwrap();
            Self {
                root,
                backend,
                table: HashMap::new(),
                watches: HashMap::new(),
                start: Instant::now(),
            }
        }

        fn add(&mut self, mb: u64, busy: bool) -> (Uuid, Uuid, i32, Arc<PolicyDriver>) {
            let session = {
                let mut state = self.backend.task_state.lock();
                let project = state.projects[0].id;
                let mut session = state.new_session(project, ProviderKind::Codex);
                session.begin_turn("seed");
                session.provider_cursor = Some(ProviderResumeCursor::Codex {
                    thread_id: "thread".into(),
                });
                session.finish_active_turn(TurnStatus::Completed);
                if busy {
                    session.begin_turn("working");
                }
                let id = session.id;
                state.push_session(session);
                id
            };
            let runtime = Uuid::new_v4();
            let driver = Arc::new(PolicyDriver::default());
            self.backend.sessions.lock().insert(
                session,
                RuntimeEntry {
                    plan_gate: None,
                    runtime_id: runtime,
                    driver: DriverHandle::from_control(driver.clone()),
                    stable: None,
                    last_active: self.start,
                    resumable: true,
                    computer_use_available: false,
                    provider: ProviderKind::Codex,
                    cwd: self.root.clone(),
                },
            );
            self.backend
                .runtime_lifecycles
                .session(session)
                .lock()
                .set(Some(runtime));
            // Deliberately outside supported hosts' PID ranges: these synthetic
            // samples must never authorize a signal to a real process.
            let pid = i32::MAX - i32::try_from(self.table.len()).unwrap();
            self.table.insert(
                pid,
                crate::stats::ProcEntry {
                    ppid: i32::try_from(std::process::id()).unwrap(),
                    rss: mb * (1 << 20),
                    birth: 1,
                    name: "fixture".into(),
                },
            );
            std::fs::write(
                self.backend.memory_guard.anchor(runtime),
                format!("{pid} 1"),
            )
            .unwrap();
            (session, runtime, pid, driver)
        }

        fn tick(&mut self, seconds: u64, pressure: Option<Level>) {
            self.backend.memory_guard_sample(
                &mut self.watches,
                self.start + Duration::from_secs(seconds),
                &self.table,
                pressure,
            );
        }

        fn records(&self) -> Vec<serde_json::Value> {
            std::fs::read_to_string(self.root.join("memory-guard/evictions.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    #[cfg(unix)]
    impl Drop for PolicyFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[cfg(unix)]
    #[test]
    fn memory_guard_default_is_soft_but_explicit_limits_and_emergency_remain_strict() {
        use std::sync::atomic::Ordering::Relaxed;
        for (limit, pressure, expected) in [
            (None, Some(Level::Normal), false),
            (None, None, false),
            (None, Some(Level::Elevated), true),
            (Some(DEFAULT_LIMIT_MB), Some(Level::Normal), true),
            (Some(DEFAULT_LIMIT_MB), None, true),
            (Some(0), Some(Level::Critical), false),
        ] {
            let mut fixture = PolicyFixture::new(limit);
            let (session, _, _, driver) = fixture.add(DEFAULT_LIMIT_MB + 1, true);
            for seconds in (0..=30).step_by(5) {
                fixture.tick(seconds, pressure);
                assert_eq!(
                    driver.cancels.load(Relaxed),
                    usize::from(expected && seconds == 30)
                );
            }
            assert_eq!(fixture.backend.memory_guard.paused(session), expected);
            assert_eq!(fixture.records().len(), usize::from(expected));
        }
        let mut fixture = PolicyFixture::new(Some(1));
        let (_, _, _, driver) = fixture.add(2, true);
        for seconds in (0..=30).step_by(5) {
            fixture.tick(seconds, Some(Level::Critical));
        }
        assert_eq!(
            driver.cancels.load(Relaxed),
            1,
            "the emergency size floor must not suppress an explicit budget"
        );
        let mut fixture = PolicyFixture::new(None);
        let (_, _, _, driver) = fixture.add(256, true);
        fixture.tick(0, Some(Level::Critical));
        assert_eq!(
            driver.cancels.load(Relaxed),
            1,
            "critical pressure bypasses sustained overage"
        );
    }

    #[cfg(unix)]
    #[test]
    fn memory_guard_serializes_budget_breaches_and_resamples_after_exit() {
        use std::sync::atomic::Ordering::Relaxed;
        for hard_limit in [false, true] {
            let mut fixture = PolicyFixture::new(hard_limit.then_some(DEFAULT_LIMIT_MB));
            let (_, _, _, smaller) = fixture.add(5000, true);
            let (largest_session, _, largest_pid, largest) = fixture.add(6000, true);
            for seconds in (0..=35).step_by(5) {
                fixture.tick(seconds, Some(Level::Elevated));
            }
            assert_eq!(largest.cancels.load(Relaxed), 1);
            assert_eq!(
                smaller.cancels.load(Relaxed),
                0,
                "one containment at a time"
            );
            fixture.table.remove(&largest_pid);
            fixture.tick(40, Some(Level::Normal));
            fixture.tick(45, Some(Level::Normal));
            assert_eq!(
                smaller.cancels.load(Relaxed),
                0,
                "the completion sample cannot choose another victim"
            );
            fixture.tick(50, Some(Level::Normal));
            assert!(
                !fixture
                    .backend
                    .sessions
                    .lock()
                    .contains_key(&largest_session),
                "an interrupted running turn must not be reopened automatically"
            );
            assert_eq!(
                smaller.cancels.load(Relaxed),
                usize::from(hard_limit),
                "relieved pressure protects the next task unless its limit is explicit"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn memory_guard_reclaims_largest_idle_first_without_pausing_or_reopening_it() {
        use std::sync::atomic::Ordering::Relaxed;
        for relieved in [false, true] {
            let mut fixture = PolicyFixture::new(None);
            let (small_session, _, small_pid, small) = fixture.add(512, false);
            let (idle_session, _, idle_pid, idle) = fixture.add(1024, false);
            let (_, _, _, busy) = fixture.add(8000, true);
            fixture.tick(0, Some(Level::Critical));
            assert_eq!(idle.shutdowns.load(Relaxed), 1);
            assert_eq!(idle.cancels.load(Relaxed), 0);
            assert_eq!(small.shutdowns.load(Relaxed), 0);
            assert_eq!(busy.cancels.load(Relaxed), 0);
            assert!(!fixture.backend.sessions.lock().contains_key(&idle_session));
            assert!(!fixture.backend.memory_guard.paused(idle_session));
            fixture.table.remove(&idle_pid);
            let pressure = Some(if relieved {
                Level::Normal
            } else {
                Level::Critical
            });
            fixture.tick(5, pressure);
            fixture.tick(10, pressure);
            assert_eq!(small.shutdowns.load(Relaxed), 0);
            fixture.tick(15, pressure);
            assert_eq!(small.shutdowns.load(Relaxed), usize::from(!relieved));
            assert_eq!(busy.cancels.load(Relaxed), 0);
            assert!(
                !fixture.backend.sessions.lock().contains_key(&idle_session),
                "idle reclamation must not immediately reopen a provider"
            );
            if !relieved {
                fixture.table.remove(&small_pid);
                fixture.tick(20, pressure);
                fixture.tick(25, pressure);
                assert_eq!(busy.cancels.load(Relaxed), 0);
                fixture.tick(30, pressure);
                assert_eq!(
                    busy.cancels.load(Relaxed),
                    1,
                    "critical pressure can interrupt active work after idle reclamation"
                );
            } else {
                assert!(fixture.backend.sessions.lock().contains_key(&small_session));
            }
            let records = fixture.records();
            assert_eq!(records[0]["eviction"]["reason"], "idle_pressure");
            assert!(
                records
                    .iter()
                    .any(|r| r["event"] == "completed" && r["outcome"] == "exited_after_shutdown")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn memory_guard_idle_reclamation_rechecks_activity_and_supports_unregistered_drivers() {
        use std::sync::atomic::Ordering::Relaxed;
        let mut fixture = PolicyFixture::new(None);
        let (session, runtime, _, idle) = fixture.add(512, false);
        let (_, _, _, busy) = fixture.add(8000, true);
        std::fs::remove_file(fixture.backend.memory_guard.anchor(runtime)).unwrap();
        assert!(
            fixture
                .backend
                .take_idle_runtime(session, Uuid::new_v4())
                .is_none(),
            "a replacement generation cannot be removed"
        );
        {
            let mut state = fixture.backend.task_state.lock();
            state
                .sessions
                .iter_mut()
                .find(|s| s.id == session)
                .unwrap()
                .begin_turn("new prompt");
        }
        assert!(
            fixture
                .backend
                .take_idle_runtime(session, runtime)
                .is_none(),
            "a task that started working after selection is protected"
        );
        {
            let mut state = fixture.backend.task_state.lock();
            state
                .sessions
                .iter_mut()
                .find(|s| s.id == session)
                .unwrap()
                .finish_active_turn(TurnStatus::Completed);
        }
        fixture.tick(0, Some(Level::Critical));
        assert_eq!(idle.shutdowns.load(Relaxed), 1);
        assert_eq!(busy.cancels.load(Relaxed), 0);
        assert_eq!(fixture.records()[1]["outcome"], "unobserved");
        assert!(!fixture.backend.memory_guard.paused(session));

        let mut fixture = PolicyFixture::new(None);
        let mut settings = fixture.backend.settings.get();
        settings.runtime_idle_timeout_secs = Some(0);
        fixture.backend.settings.replace(settings).unwrap();
        let (_, _, _, idle) = fixture.add(512, false);
        fixture.tick(0, Some(Level::Elevated));
        assert_eq!(
            idle.shutdowns.load(Relaxed),
            0,
            "idle eviction opt-out is preserved"
        );
    }

    #[test]
    fn memory_guard_requires_continuous_overage_and_restarts_the_clock_after_a_sample_gap() {
        let start = Instant::now();
        let mut sustained = Sustained::default();
        for seconds in (0..30).step_by(5) {
            assert!(!sustained.observe(start + Duration::from_secs(seconds), true));
        }
        assert!(sustained.observe(start + OVERAGE, true));
        assert!(!sustained.observe(start + Duration::from_secs(35), false));
        assert!(!sustained.observe(start + Duration::from_secs(40), true));
        assert!(
            !sustained.observe(start + Duration::from_secs(70), true),
            "missing samples cannot establish sustained usage"
        );
    }

    #[test]
    fn memory_guard_pause_and_single_restart_budget_survive_daemon_replacement() {
        let root = directory();
        let session = Uuid::new_v4();
        {
            let guard = MemoryGuard::open(&root).unwrap();
            guard.pause(session).unwrap();
            assert!(guard.check_admission(session).is_err());
            assert!(
                guard.user_resume(session).is_err(),
                "cleanup must finish first"
            );
            guard.stopping.lock().remove(&session);
            assert!(guard.claim_restart(session).unwrap());
            assert!(guard.check_admission(session).is_ok());
        }
        let guard = MemoryGuard::open(&root).unwrap();
        guard.pause(session).unwrap();
        guard.stopping.lock().remove(&session);
        assert!(!guard.claim_restart(session).unwrap());
        assert!(guard.paused(session));
        guard.user_resume(session).unwrap();
        assert!(guard.check_admission(session).is_ok());
        guard
            .critical
            .store(true, std::sync::atomic::Ordering::Release);
        assert!(guard.check_admission(Uuid::new_v4()).is_err());
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn memory_guard_failed_recovery_write_keeps_admission_closed() {
        let root = directory();
        let guard = MemoryGuard::open(&root).unwrap();
        let session = Uuid::new_v4();
        guard.pause(session).unwrap();
        guard.stopping.lock().remove(&session);
        std::fs::remove_file(guard.directory.join("recovery.json")).unwrap();
        std::fs::create_dir(guard.directory.join("recovery.json")).unwrap();
        assert!(guard.claim_restart(session).is_err());
        assert!(guard.paused(session));
        assert!(guard.user_resume(session).is_err());
        assert!(guard.paused(session));
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn memory_guard_tracks_descendants_after_reparenting_and_rejects_pid_reuse() {
        let entry = |ppid, birth, rss| crate::stats::ProcEntry {
            ppid,
            birth,
            rss,
            name: String::new(),
        };
        let mut table = HashMap::from([
            (100, entry(1, 10, 1)),
            (101, entry(100, 11, 4)),
            (102, entry(101, 12, 8)),
            (200, entry(1, 20, 1000)),
        ]);
        let mut owned = HashSet::from([Process {
            pid: 100,
            birth: 10,
        }]);
        assert_eq!(refresh_tree(&mut owned, &table), 13);
        table.remove(&100);
        table.get_mut(&101).unwrap().ppid = 1;
        assert_eq!(refresh_tree(&mut owned, &table), 12);
        table.insert(101, entry(1, 99, 2000));
        assert_eq!(refresh_tree(&mut owned, &table), 8);
        assert!(!owned.contains(&Process {
            pid: 101,
            birth: 99
        }));
        assert!(!owned.iter().any(|p| p.pid == 200));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn memory_guard_accepts_only_the_acp_guardians_exact_launch_marker() {
        use std::os::unix::process::CommandExt as _;
        use std::process::{Child, Command as ProcessCommand, Stdio};
        struct OwnedGroup(Child);
        impl Drop for OwnedGroup {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    // SAFETY: this live, unreaped child leads the process group we created.
                    unsafe {
                        libc::kill(-i32::try_from(self.0.id()).unwrap(), libc::SIGKILL);
                    }
                }
                let _ = self.0.wait();
            }
        }
        let root = directory();
        let guard = MemoryGuard::open(&root).unwrap();
        let path = guard.anchor(Uuid::new_v4());
        let marker = format!("goddard-runtime:{}", path.display());
        let process = OwnedGroup(
            ProcessCommand::new("/bin/sh")
                .args([
                    "-c",
                    crate::command_env::DAEMON_GUARDIAN_SCRIPT,
                    &marker,
                    "/bin/sleep",
                    "600",
                ])
                .env("GODDARD_PROCESS_ANCHOR", &path)
                .process_group(0)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let identity = loop {
            if let Some(identity) =
                registered_process(&path, &crate::stats::process_table().unwrap())
            {
                break identity;
            }
            assert!(
                Instant::now() < deadline,
                "guardian registration must be attributable to its live process"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(identity.pid, i32::try_from(process.0.id()).unwrap());
        let other_launch = guard.anchor(Uuid::new_v4());
        std::fs::write(&other_launch, process.0.id().to_string()).unwrap();
        assert!(
            registered_process(&other_launch, &crate::stats::process_table().unwrap()).is_none()
        );
        std::fs::write(
            &other_launch,
            format!("{} {}", process.0.id(), identity.birth + 1),
        )
        .unwrap();
        assert!(
            registered_process(&other_launch, &crate::stats::process_table().unwrap()).is_none()
        );
        drop(process);
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn memory_guard_contains_a_busy_process_tree_without_killing_a_sibling_or_replaying_work() {
        use std::os::unix::process::CommandExt as _;
        use std::process::{Child, Command as ProcessCommand, Stdio};
        struct OwnedChild(Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    // SAFETY: this live, unreaped child leads the process group we created.
                    unsafe {
                        libc::kill(-i32::try_from(self.0.id()).unwrap(), libc::SIGKILL);
                    }
                }
                let _ = self.0.wait();
            }
        }
        #[derive(Default)]
        struct Driver(std::sync::atomic::AtomicUsize);
        impl driver::DriverControl for Driver {
            fn prompt(&self, _: String) {
                panic!("interrupted work must not replay");
            }
            fn cancel(&self) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            fn respond(&self, _: String, _: String) {}
            fn rollback(&self, _: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
                Ok(None)
            }
        }
        for emergency in [false, true] {
            let root = directory();
            let pid_path = root.join("child.pid");
            let mut process = OwnedChild(
                ProcessCommand::new("/bin/sh")
                    .args([
                        "-c",
                        "trap '' TERM; sleep 600 & child=$!; printf '%s' \"$child\" > \"$1\"; wait",
                        "memory-guard-test",
                    ])
                    .arg(&pid_path)
                    .process_group(0)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let mut sibling = OwnedChild(
                ProcessCommand::new("sleep")
                    .arg("600")
                    .process_group(0)
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(3);
            let descendant = loop {
                if let Ok(text) = std::fs::read_to_string(&pid_path)
                    && let Ok(pid) = text.parse::<i32>()
                {
                    break pid;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            };
            let (backend, session) = super::super::tests::surface_test_backend(&root);
            let mut settings = backend.settings.get();
            settings.agent_memory_limit_mb = Some(DEFAULT_LIMIT_MB);
            backend.settings.replace(settings).unwrap();
            {
                let mut state = backend.task_state.lock();
                state
                    .sessions
                    .iter_mut()
                    .find(|s| s.id == session)
                    .unwrap()
                    .begin_turn("busy work");
            }
            let runtime = Uuid::new_v4();
            let driver = Arc::new(Driver::default());
            backend.sessions.lock().insert(
                session,
                RuntimeEntry {
                    plan_gate: None,
                    runtime_id: runtime,
                    driver: DriverHandle::from_control(driver.clone()),
                    stable: None,
                    last_active: Instant::now(),
                    resumable: true,
                    computer_use_available: false,
                    provider: ProviderKind::Codex,
                    cwd: root.clone(),
                },
            );
            backend
                .runtime_lifecycles
                .session(session)
                .lock()
                .set(Some(runtime));
            let sibling_session = {
                let mut state = backend.task_state.lock();
                let project = state.projects[0].id;
                let mut sibling = state.new_session(project, ProviderKind::Codex);
                sibling.begin_turn("independent busy work");
                let id = sibling.id;
                state.push_session(sibling);
                id
            };
            let sibling_runtime = Uuid::new_v4();
            let sibling_driver = Arc::new(Driver::default());
            backend.sessions.lock().insert(
                sibling_session,
                RuntimeEntry {
                    plan_gate: None,
                    runtime_id: sibling_runtime,
                    driver: DriverHandle::from_control(sibling_driver.clone()),
                    stable: None,
                    last_active: Instant::now(),
                    resumable: false,
                    computer_use_available: false,
                    provider: ProviderKind::Codex,
                    cwd: root.clone(),
                },
            );
            let sibling_pid = i32::try_from(sibling.0.id()).unwrap();
            let sibling_birth = waku_base::process_identity::start_time(sibling_pid).unwrap();
            std::fs::write(
                backend.memory_guard.anchor(sibling_runtime),
                format!("{sibling_pid} {sibling_birth}"),
            )
            .unwrap();
            let pid = i32::try_from(process.0.id()).unwrap();
            let birth = waku_base::process_identity::start_time(pid).unwrap();
            std::fs::write(
                backend.memory_guard.anchor(runtime),
                format!("{pid} {birth}"),
            )
            .unwrap();
            let mut table = crate::stats::process_table().unwrap();
            table.get_mut(&pid).unwrap().rss = (DEFAULT_LIMIT_MB + 1) * (1 << 20);
            table.get_mut(&sibling_pid).unwrap().rss = 1024 * (1 << 20);
            assert!(table.contains_key(&descendant));
            let start = Instant::now();
            let mut watches = HashMap::new();
            for seconds in (0..=if emergency { 0 } else { 30 }).step_by(5) {
                backend.memory_guard_sample(
                    &mut watches,
                    start + Duration::from_secs(seconds),
                    &table,
                    Some(if emergency {
                        Level::Critical
                    } else {
                        Level::Normal
                    }),
                );
            }
            assert_eq!(backend.memory_guard.host_critical(), emergency);
            assert_eq!(
                sibling_driver.0.load(std::sync::atomic::Ordering::Relaxed),
                0
            );
            assert_eq!(driver.0.load(std::sync::atomic::Ordering::Relaxed), 1);
            assert!(backend.memory_guard.paused(session));
            assert!(backend.memory_guard.user_resume(session).is_err());
            backend.memory_guard_sample(
                &mut watches,
                start + Duration::from_secs(40),
                &table,
                Some(Level::Normal),
            );
            backend.memory_guard_sample(
                &mut watches,
                start + Duration::from_secs(45),
                &table,
                Some(Level::Normal),
            );
            let deadline = Instant::now() + Duration::from_secs(3);
            while process.0.try_wait().unwrap().is_none() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(sibling.0.try_wait().unwrap().is_none());
            assert!(backend.sessions.lock().get(&session).is_none());
            let deadline = Instant::now() + Duration::from_secs(3);
            let table = loop {
                let table = crate::stats::process_table().unwrap();
                if !table.contains_key(&descendant) {
                    break table;
                }
                assert!(
                    Instant::now() < deadline,
                    "the owned descendant must exit too"
                );
                std::thread::sleep(Duration::from_millis(5));
            };
            backend.memory_guard_sample(
                &mut watches,
                start + Duration::from_secs(50),
                &table,
                Some(Level::Normal),
            );
            backend.memory_guard_sample(
                &mut watches,
                start + Duration::from_secs(55),
                &table,
                Some(Level::Normal),
            );
            let records: Vec<serde_json::Value> =
                std::fs::read_to_string(backend.memory_guard.directory.join("evictions.jsonl"))
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
            let started: Vec<_> = records.iter().filter(|r| r["event"] == "started").collect();
            let completed: Vec<_> = records
                .iter()
                .filter(|r| r["event"] == "completed")
                .collect();
            assert_eq!(started.len(), 1);
            assert_eq!(completed.len(), 1);
            assert_eq!(
                started[0]["eviction"]["incident"],
                completed[0]["eviction"]["incident"]
            );
            assert_eq!(
                started[0]["eviction"]["reason"],
                if emergency {
                    "critical_pressure"
                } else {
                    "memory_limit"
                }
            );
            assert_eq!(completed[0]["outcome"], "force_killed");
            assert!(
                records
                    .iter()
                    .any(|r| r["event"] == "escalated" && r["outcome"] == "force_killed")
            );
            assert!(
                backend.memory_guard.paused(session),
                "busy work requires explicit review"
            );
            backend.memory_guard.user_resume(session).unwrap();
            assert!(backend.memory_guard.check_admission(session).is_ok());
            drop(backend);
            drop(process);
            drop(sibling);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
