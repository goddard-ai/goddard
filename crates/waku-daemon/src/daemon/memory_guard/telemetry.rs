//! Local, bounded eviction history. Starts count interventions; completions
//! describe the last successful escalation before the captured tree disappeared,
//! not a kernel-confirmed cause of death. Missing completions remain unknown.
use super::*;

const LOG_CAP: u64 = 4 * 1024 * 1024;
const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Reason {
    CriticalPressure,
    MemoryLimit,
    IdlePressure,
    IdleTimeout,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    Cancelled,
    Terminated,
    ForceKilled,
    ExitedAfterShutdown,
    Unobserved,
    ObservationTimedOut,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Eviction {
    incident: Uuid,
    task: Uuid,
    runtime: Uuid,
    reason: Reason,
    started_at: u64,
    bytes: Option<u64>,
    limit_mb: Option<u64>,
    process_count: usize,
    processes: Vec<i32>,
    #[serde(skip)]
    started: Instant,
}

impl Eviction {
    pub(super) fn new(
        task: Uuid,
        runtime: Uuid,
        reason: Reason,
        bytes: Option<u64>,
        limit_mb: Option<u64>,
        owned: &HashSet<Process>,
        started: Instant,
    ) -> Self {
        let mut processes: Vec<_> = owned.iter().map(|p| p.pid).collect();
        processes.sort_unstable();
        processes.truncate(64);
        Self {
            incident: Uuid::new_v4(),
            task,
            runtime,
            reason,
            started_at: crate::model::unix_time(),
            bytes,
            limit_mb,
            process_count: owned.len(),
            processes,
            started,
        }
    }
}

pub(super) struct IdleEviction {
    incident: Eviction,
    owned: HashSet<Process>,
}

impl MemoryGuard {
    pub(super) fn record_eviction(
        &self,
        incident: &Eviction,
        event: &str,
        outcome: Option<Outcome>,
        now: Instant,
    ) {
        let record = serde_json::json!({
            "schemaVersion": 1,
            "at": crate::model::unix_time(),
            "event": event,
            "eviction": incident,
            "outcome": outcome,
            "elapsedMs": now.saturating_duration_since(incident.started).as_millis(),
        });
        // The reaper and guard write concurrently; rotation must be serialized
        // with appends to avoid losing a writer's record during truncation.
        let _writer = self.eviction_writer.lock();
        let result = (|| -> anyhow::Result<()> {
            let mut line = serde_json::to_vec(&record)?;
            line.push(b'\n');
            waku_base::fs_ext::append_capped(
                &self.directory.join("evictions.jsonl"),
                &line,
                LOG_CAP,
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("memory guard could not write eviction telemetry: {error:#}");
        }
    }

    /// Capture identities before shutdown. Observation never authorizes signals.
    pub(in super::super) fn track_idle_eviction(
        &self,
        task: Uuid,
        runtime: Uuid,
        pressure: bool,
        table: Option<&HashMap<i32, crate::stats::ProcEntry>>,
    ) {
        let now = Instant::now();
        let mut owned = table
            .and_then(|table| registered_process(&self.anchor(runtime), table))
            .map(|root| HashSet::from([root]))
            .unwrap_or_default();
        let bytes = table
            .filter(|_| !owned.is_empty())
            .map(|table| refresh_tree(&mut owned, table));
        let reason = if pressure {
            Reason::IdlePressure
        } else {
            Reason::IdleTimeout
        };
        let incident = Eviction::new(task, runtime, reason, bytes, None, &owned, now);
        self.record_eviction(&incident, "started", None, now);
        if owned.is_empty() || !cfg!(any(target_os = "macos", target_os = "linux")) {
            self.record_eviction(&incident, "completed", Some(Outcome::Unobserved), now);
        } else {
            self.idle_evictions
                .lock()
                .push(IdleEviction { incident, owned });
        }
    }

    pub(super) fn observe_idle_evictions(
        &self,
        now: Instant,
        table: Option<&HashMap<i32, crate::stats::ProcEntry>>,
    ) {
        self.idle_evictions.lock().retain_mut(|pending| {
            // A reaper can enqueue after this guard tick began collecting its
            // table. That older snapshot cannot establish the new tree's exit.
            if now < pending.incident.started {
                return true;
            }
            if let Some(table) = table {
                refresh_tree(&mut pending.owned, table);
                if pending.owned.is_empty() {
                    self.record_eviction(
                        &pending.incident,
                        "completed",
                        Some(Outcome::ExitedAfterShutdown),
                        now,
                    );
                    return false;
                }
            }
            if now.saturating_duration_since(pending.incident.started) >= OBSERVATION_TIMEOUT {
                self.record_eviction(
                    &pending.incident,
                    "completed",
                    Some(Outcome::ObservationTimedOut),
                    now,
                );
                return false;
            }
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records(guard: &MemoryGuard) -> Vec<serde_json::Value> {
        std::fs::read_to_string(guard.directory.join("evictions.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn idle_eviction_telemetry_distinguishes_exit_timeout_and_missing_observation() {
        let root = std::env::temp_dir().join(format!("eviction-telemetry-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let guard = MemoryGuard::open(&root).unwrap();
        let task = Uuid::new_v4();
        let runtime = Uuid::new_v4();
        let pid = 424242;
        std::fs::write(guard.anchor(runtime), format!("{pid} 1")).unwrap();
        let mut table = HashMap::from([(
            pid,
            crate::stats::ProcEntry {
                ppid: i32::try_from(std::process::id()).unwrap(),
                rss: 1024,
                birth: 1,
                name: "test".into(),
            },
        )]);
        let before = Instant::now() - Duration::from_secs(1);
        guard.track_idle_eviction(task, runtime, true, Some(&table));
        assert_eq!(
            records(&guard).len(),
            1,
            "shutdown request is not a completed exit"
        );
        let now = Instant::now();
        guard.observe_idle_evictions(before, Some(&HashMap::new()));
        assert_eq!(
            records(&guard).len(),
            1,
            "a snapshot predating eviction is not an exit"
        );
        guard.observe_idle_evictions(now, None);
        assert_eq!(records(&guard).len(), 1, "failed sample is not an exit");
        // A new process reusing the PID must not keep the old eviction open.
        table.get_mut(&pid).unwrap().birth = 2;
        guard.observe_idle_evictions(now, Some(&table));
        guard.observe_idle_evictions(now, Some(&table));
        let rows = records(&guard);
        assert_eq!(rows.len(), 2, "completion is written once");
        assert_eq!(rows[0]["eviction"]["reason"], "idle_pressure");
        assert_eq!(rows[0]["eviction"]["bytes"], 1024);
        assert_eq!(
            rows[0]["eviction"]["incident"],
            rows[1]["eviction"]["incident"]
        );
        assert_eq!(rows[1]["outcome"], "exited_after_shutdown");

        table.get_mut(&pid).unwrap().birth = 1;
        guard.track_idle_eviction(task, runtime, false, Some(&table));
        guard.observe_idle_evictions(Instant::now() + OBSERVATION_TIMEOUT, None);
        let rows = records(&guard);
        assert_eq!(rows[2]["eviction"]["reason"], "idle_timeout");
        assert_eq!(rows[3]["outcome"], "observation_timed_out");

        guard.track_idle_eviction(task, Uuid::new_v4(), true, None);
        let rows = records(&guard);
        assert_eq!(rows[5]["outcome"], "unobserved");
        assert_eq!(rows[5]["eviction"]["bytes"], serde_json::Value::Null);
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }
}
