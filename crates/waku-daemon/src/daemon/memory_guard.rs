use super::*;
use std::time::{Duration, Instant};

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
    fn signal(self, signal: i32) {
        if self.pid > 1 && waku_base::process_identity::start_time(self.pid) == Some(self.birth) {
            // SAFETY: pid is positive, identifies a captured owned process, and its
            // birth stamp was rechecked immediately before this signal.
            unsafe {
                libc::kill(self.pid, signal);
            }
        }
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
        let critical = crate::pressure::critical().unwrap_or(false);
        self.memory_guard
            .critical
            .store(critical, std::sync::atomic::Ordering::Release);
        let Some(table) = crate::stats::process_table() else {
            return;
        };
        self.memory_guard_sample(watches, now, &table, critical);
    }

    #[cfg(unix)]
    fn memory_guard_sample(
        &self,
        watches: &mut HashMap<Uuid, Watch>,
        now: Instant,
        table: &HashMap<i32, crate::stats::ProcEntry>,
        critical: bool,
    ) {
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
                    },
                );
            }
        }
        let limit_mb = self
            .settings
            .get()
            .agent_memory_limit_mb
            .unwrap_or(DEFAULT_LIMIT_MB);
        let limit = limit_mb.saturating_mul(1 << 20);
        for watch in watches.values_mut() {
            watch.bytes = refresh_tree(&mut watch.owned, &table);
        }
        // One emergency victim at a time lets its cancellation reclaim memory
        // before another task is interrupted. Budget breaches remain independent.
        let containing = watches.values().any(|w| {
            matches!(
                w.phase,
                Phase::Cancelling { .. } | Phase::Terminating { .. }
            )
        });
        let victim = (critical && !containing && limit != 0)
            .then(|| {
                watches
                    .iter()
                    .filter(|(_, w)| {
                        matches!(w.phase, Phase::Watching) && w.bytes >= 256 * (1 << 20)
                    })
                    .max_by_key(|(_, w)| w.bytes)
                    .map(|(&id, _)| id)
            })
            .flatten();
        let mut retire = Vec::new();
        for (&runtime, watch) in watches.iter_mut() {
            match watch.phase {
                Phase::Watching => {
                    if watch.owned.is_empty() {
                        retire.push(runtime);
                        continue;
                    }
                    let sustained = watch
                        .sustained
                        .observe(now, limit != 0 && watch.bytes > limit);
                    if !sustained && victim != Some(runtime) {
                        continue;
                    }
                    let lifecycle = self.runtime_lifecycles.session(watch.session);
                    let _owner = lifecycle.lock();
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
                        for process in &watch.owned {
                            process.signal(libc::SIGTERM);
                        }
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
                    let reason = if victim == Some(runtime) {
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
                    for process in &watch.owned {
                        process.signal(libc::SIGTERM);
                    }
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
                        watch.phase = Phase::Stopped;
                    } else if now.saturating_duration_since(at) >= TERM_GRACE {
                        for process in &watch.owned {
                            process.signal(libc::SIGKILL);
                        }
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
            let sibling_session = Uuid::new_v4();
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
                    emergency,
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
                false,
            );
            backend.memory_guard_sample(
                &mut watches,
                start + Duration::from_secs(45),
                &table,
                false,
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
                false,
            );
            backend.memory_guard_sample(
                &mut watches,
                start + Duration::from_secs(55),
                &table,
                false,
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
