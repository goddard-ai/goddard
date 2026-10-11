//! Daemon-owned scheduled automations: definitions, run history, and the
//! tick that fires them.
//!
//! The daemon owns the whole lifecycle so schedules fire whether or not a
//! desktop is attached. [`AutomationService`] keeps the document behind one
//! mutex — definitions plus a bounded run history — persists it to
//! `automations.json` beside the task database, and broadcasts the whole
//! document on every change. A tick thread evaluates due schedules on a
//! fixed cadence. Scheduled occurrence identities survive restart and history
//! retirement. Interrupted work is failed conservatively, never replayed;
//! this is not a transaction with the task store or an exactly-once guarantee.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex as StdMutex, Weak};
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};
use chrono::{DateTime, TimeZone, Utc};
use cron::Schedule;
use parking_lot::Mutex;
use subtle::ConstantTimeEq as _;
use uuid::Uuid;

use waku_protocol::AgentWorkspace;
use waku_protocol::automations::{
    Automation, AutomationInput, AutomationPrecheck, AutomationPrecheckResult, AutomationRun,
    AutomationRunStatus, AutomationSchedule, AutomationTrigger, AutomationWorkspace,
    AutomationsState,
};
use waku_protocol::model::unix_time;

use crate::daemon::{AgentCreateSelection, WakuBackend};
use crate::model::DriverEvent;
use crate::server::EventSink;

pub use waku_server::AutomationsSink;

/// One scheduler pass every this often. Sub-minute schedules would need a
/// faster cadence; the tick itself is cheap, the bound is scheduling jitter.
const TICK_INTERVAL: Duration = Duration::from_secs(45);
/// Terminal history retained when accepting a new occurrence, in addition
/// to in-flight runs. Includes the newest scheduled retirement watermark.
const MAX_RUNS_PER_AUTOMATION: usize = 50;
/// A running run whose turn never reports gets this long before the tick
/// fails it; providers can legitimately work for hours, so the bound stays
/// generous.
const STALE_RUN_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;

/// JSON persistence for the automations document — same quarantine +
/// atomic-write pattern as [`crate::settings::DaemonSettingsStore`].
pub struct AutomationsStore {
    path: PathBuf,
    #[cfg(test)]
    before_save: Mutex<Option<Arc<dyn Fn(&AutomationsState) -> io::Result<()> + Send + Sync>>>,
}

impl AutomationsStore {
    fn open(path: PathBuf) -> anyhow::Result<(Self, AutomationsState)> {
        let state = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(state) => state,
                Err(error) => {
                    let backup = quarantine_corrupt(&path)?;
                    eprintln!(
                        "Goddard daemon moved invalid automations to {}: {error}",
                        backup.display()
                    );
                    AutomationsState::default()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => AutomationsState::default(),
            Err(error) => return Err(error.into()),
        };
        Ok((
            Self {
                path,
                #[cfg(test)]
                before_save: Mutex::new(None),
            },
            state,
        ))
    }

    fn save(&self, state: &AutomationsState) -> io::Result<()> {
        #[cfg(test)]
        if let Some(before_save) = self.before_save.lock().clone() {
            before_save(state)?;
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(state)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, data)?;
        fs::rename(temporary, &self.path)
    }
}

fn quarantine_corrupt(path: &Path) -> io::Result<PathBuf> {
    let extension = format!("json.corrupt-{}", Uuid::new_v4().simple());
    let backup = path.with_extension(extension);
    fs::rename(path, &backup)?;
    Ok(backup)
}

/// Only `committed` is acknowledged. A failed consequential write retains
/// `repair`, which can advance but can never become dispatchable work again.
struct Lifecycle {
    committed: AutomationsState,
    repair: Option<AutomationsState>,
    reconcile: bool,
    dispatched: HashSet<Uuid>,
}

/// Definition edits, run bookkeeping, and the scheduler tick.
pub struct AutomationService {
    lifecycle: Mutex<Lifecycle>,
    store: AutomationsStore,
    sink: Mutex<Option<AutomationsSink>>,
    /// Set when the scheduler starts; dispatch upgrades it to reach the
    /// session-creation path. `None`/dead weakref fails runs as unavailable.
    backend: Mutex<Weak<WakuBackend>>,
    /// Root sink the server installs so scheduler-initiated sessions emit
    /// events to subscribers the way request-initiated ones do.
    event_source: Mutex<Option<EventSink>>,
    /// The `TaskStateChanged` bump — daemon-initiated task mutations bypass
    /// the request dispatcher's per-command bump, so dispatch rings it
    /// itself or clients never learn the run's task exists.
    task_notifier: Mutex<Option<crate::share::TaskNotifier>>,
    /// Re-entry guard: a slow dispatch (precheck timeout, worktree create)
    /// must not let the next tick re-evaluate the same due occurrence.
    evaluating: AtomicBool,
    started: AtomicBool,
    #[cfg(test)]
    dispatch_hook: Mutex<Option<Arc<dyn Fn(Uuid) + Send + Sync>>>,
    #[cfg(test)]
    fail_worker: AtomicBool,
    stop: Arc<(StdMutex<bool>, Condvar)>,
}

impl AutomationService {
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let (store, state) = AutomationsStore::open(path)?;
        Ok(Self {
            lifecycle: Mutex::new(Lifecycle {
                reconcile: state.runs.iter().any(|run| !run.status.is_terminal()),
                committed: state,
                repair: None,
                dispatched: HashSet::new(),
            }),
            store,
            sink: Mutex::new(None),
            backend: Mutex::new(Weak::new()),
            event_source: Mutex::new(None),
            task_notifier: Mutex::new(None),
            evaluating: AtomicBool::new(false),
            started: AtomicBool::new(false),
            #[cfg(test)]
            dispatch_hook: Mutex::new(None),
            #[cfg(test)]
            fail_worker: AtomicBool::new(false),
            stop: Arc::new((StdMutex::new(false), Condvar::new())),
        })
    }

    pub fn document(&self) -> AutomationsState {
        Self::visible_document(&self.lifecycle.lock())
    }

    #[cfg(test)]
    pub(crate) fn accept_test_runs(&self, runs: Vec<AutomationRun>) -> anyhow::Result<()> {
        self.transition(|document| {
            document.runs.extend(runs);
            Ok(())
        })
    }

    /// Install the broadcast channel; `serve` does this once the hub exists.
    pub fn set_sink(&self, sink: AutomationsSink) {
        *self.sink.lock() = Some(sink);
    }

    /// Install the root event sink for scheduler-initiated dispatches.
    pub fn set_event_source(&self, events: EventSink) {
        *self.event_source.lock() = Some(events);
    }

    /// Install the `TaskStateChanged` notifier `serve` wires to the hub.
    pub fn set_task_notifier(&self, notifier: crate::share::TaskNotifier) {
        *self.task_notifier.lock() = Some(notifier);
    }

    /// Tell subscribers task state changed. Without it a dispatched run's
    /// task never appears in a client's sidebar and its transcript stays
    /// unreachable until the next client-driven reload.
    fn notify_tasks_changed(&self) {
        let notifier = self.task_notifier.lock().clone();
        if let Some(notifier) = notifier {
            notifier();
        }
    }

    /// Save, install and publish under the same lease. Sinks must only
    /// enqueue the snapshot; they must not call back into this service.
    fn install(
        &self,
        lifecycle: &mut Lifecycle,
        candidate: AutomationsState,
    ) -> anyhow::Result<()> {
        self.store
            .save(&candidate)
            .context("could not persist automation lifecycle")?;
        lifecycle
            .dispatched
            .retain(|id| candidate.runs.iter().any(|run| run.id == *id));
        lifecycle.committed = candidate;
        lifecycle.repair = None;
        self.publish_snapshot(lifecycle.committed.clone());
        Ok(())
    }

    fn publish_snapshot(&self, document: AutomationsState) {
        if let Some(sink) = self.sink.lock().clone() {
            sink(document);
        }
    }

    /// Expose uncertainty without presenting the unsaved verdict as durable.
    fn visible_document(lifecycle: &Lifecycle) -> AutomationsState {
        let mut document = lifecycle.committed.clone();
        if let Some(repair) = &lifecycle.repair {
            for run in &mut document.runs {
                let candidate = repair.runs.iter().find(|candidate| candidate.id == run.id);
                if candidate.is_some_and(|candidate| candidate != run)
                    || (candidate.is_none() && !run.status.is_terminal())
                {
                    run.error = Some("automation bookkeeping could not be saved; outcome is uncertain; dispatch will not be repeated".into());
                }
            }
        }
        document
    }

    fn ready(&self, lifecycle: &mut Lifecycle) -> anyhow::Result<()> {
        if let Some(candidate) = lifecycle.repair.clone() {
            self.install(lifecycle, candidate)?;
        }
        if lifecycle.reconcile {
            let mut candidate = lifecycle.committed.clone();
            let interrupted: Vec<_> = candidate
                .runs
                .iter()
                .filter(|run| !run.status.is_terminal())
                .map(|run| run.id)
                .collect();
            for run_id in interrupted {
                Self::settle(
                    &mut candidate,
                    run_id,
                    AutomationRunStatus::Failed,
                    Some(
                        "the daemon restarted before the run finished; work will not be replayed"
                            .into(),
                    ),
                    None,
                    "restart:interrupted",
                    unix_time(),
                );
            }
            if let Err(error) = self.install(lifecycle, candidate.clone()) {
                lifecycle.repair = Some(candidate);
                self.publish_snapshot(Self::visible_document(lifecycle));
                return Err(
                    error.context("automation restart reconciliation failed; dispatch inhibited")
                );
            }
            lifecycle.reconcile = false;
        }
        Ok(())
    }

    /// Acceptance/definition changes are discarded on failure. Nothing from
    /// the candidate is visible or dispatchable until the save succeeds.
    fn transition<T>(
        &self,
        mutate: impl FnOnce(&mut AutomationsState) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut lifecycle = self.lifecycle.lock();
        self.ready(&mut lifecycle)?;
        let mut candidate = lifecycle.committed.clone();
        let result = mutate(&mut candidate)?;
        if candidate != lifecycle.committed {
            self.install(&mut lifecycle, candidate)?;
        }
        Ok(result)
    }

    /// Work may already have happened. Retain the candidate on failure and
    /// only retry writes, never provider dispatch. Later events mutate this
    /// candidate so a failed running save cannot hide a terminal event.
    fn bookkeeping(&self, mutate: impl FnOnce(&mut AutomationsState)) -> anyhow::Result<()> {
        let mut lifecycle = self.lifecycle.lock();
        let mut candidate = lifecycle
            .repair
            .clone()
            .unwrap_or_else(|| lifecycle.committed.clone());
        mutate(&mut candidate);
        if candidate == lifecycle.committed && lifecycle.repair.is_none() {
            return Ok(());
        }
        if let Err(error) = self.install(&mut lifecycle, candidate.clone()) {
            lifecycle.repair = Some(candidate);
            self.publish_snapshot(Self::visible_document(&lifecycle));
            return Err(error);
        }
        Ok(())
    }

    fn report_bookkeeping(&self, run_id: Uuid, result: anyhow::Result<()>) {
        if let Err(error) = result {
            eprintln!(
                "goddard-daemon automation run {run_id} bookkeeping uncertain; retrying persistence only: {error:#}"
            );
        }
    }

    /// Create or replace one automation. Validates the schedule, timezone,
    /// and target up front so a broken definition can never sit enabled and
    /// silently do nothing.
    pub fn upsert(&self, input: AutomationInput) -> anyhow::Result<Automation> {
        validate_input(&input)?;
        let now = unix_time();
        self.transition(|document| {
            let automation = match input.id.and_then(|id| {
                document
                    .automations
                    .iter_mut()
                    .find(|automation| automation.id == id)
            }) {
                Some(existing) => {
                    existing.name = input.name;
                    existing.prompt = input.prompt;
                    existing.provider = input.provider;
                    existing.model = input.model;
                    existing.project_path = input.project_path;
                    existing.workspace = input.workspace;
                    existing.base_branch = input.base_branch;
                    existing.session_id = input.session_id;
                    existing.schedule = input.schedule;
                    existing.webhook_secret = input.webhook.then(|| {
                        existing
                            .webhook_secret
                            .clone()
                            .unwrap_or_else(new_webhook_secret)
                    });
                    existing.timezone = input.timezone;
                    existing.enabled = input.enabled;
                    existing.precheck = input.precheck;
                    existing.missed_run_grace_minutes = input.missed_run_grace_minutes;
                    existing.reuse_session = input.reuse_session;
                    // Editing the schedule re-arms the next occurrence; history
                    // and the last session survive.
                    existing.updated_at = now;
                    existing.clone()
                }
                None => {
                    let automation = Automation {
                        id: Uuid::new_v4(),
                        name: input.name,
                        prompt: input.prompt,
                        provider: input.provider,
                        model: input.model,
                        project_path: input.project_path,
                        workspace: input.workspace,
                        base_branch: input.base_branch,
                        session_id: input.session_id,
                        schedule: input.schedule,
                        webhook_secret: input.webhook.then(new_webhook_secret),
                        timezone: input.timezone,
                        enabled: input.enabled,
                        precheck: input.precheck,
                        missed_run_grace_minutes: input.missed_run_grace_minutes,
                        reuse_session: input.reuse_session,
                        last_session_id: None,
                        next_run_at: None,
                        last_run_at: None,
                        last_run_status: None,
                        last_refusal_key: None,
                        created_at: now,
                        updated_at: now,
                    };
                    document.automations.push(automation.clone());
                    automation
                }
            };
            if let Some(schedule) = &automation.schedule {
                let compiled = compile_schedule(schedule)?;
                let zone = resolve_zone(&automation.timezone)?;
                let next = next_occurrence(&compiled, &zone, Utc::now());
                if let Some(entry) = document
                    .automations
                    .iter_mut()
                    .find(|entry| entry.id == automation.id)
                {
                    entry.next_run_at = next;
                }
            }
            let saved = document
                .automations
                .iter()
                .find(|entry| entry.id == automation.id)
                .cloned()
                .ok_or_else(|| anyhow!("automation vanished during upsert"))?;
            Ok(saved)
        })
    }

    /// Delete a definition and every run it ever recorded.
    pub fn remove(&self, automation_id: Uuid) -> anyhow::Result<()> {
        self.transition(|document| {
            if !document
                .automations
                .iter()
                .any(|automation| automation.id == automation_id)
            {
                bail!("automation {automation_id} is unknown to the daemon");
            }
            document
                .automations
                .retain(|automation| automation.id != automation_id);
            document
                .runs
                .retain(|run| run.automation_id != automation_id);
            Ok(())
        })
    }

    /// Queue a manual run. Returns the recorded run immediately; dispatch
    /// proceeds on a worker thread so the RPC never blocks on a precheck.
    pub fn run_now(self: &Arc<Self>, automation_id: Uuid) -> anyhow::Result<AutomationRun> {
        let automation = {
            let document = self.document();
            document
                .automations
                .iter()
                .find(|automation| automation.id == automation_id)
                .cloned()
                .ok_or_else(|| anyhow!("automation {automation_id} is unknown to the daemon"))?
        };
        let run = self.record_run(&automation, unix_time(), AutomationTrigger::Manual)?;
        self.spawn_dispatch(automation, run.id)?;
        Ok(run)
    }

    /// Fire a run from the automation's webhook URL. `Ok(None)` covers both
    /// an unknown id and a wrong key so the endpoint cannot tell them
    /// apart; a disabled automation records a refusal like a refused
    /// schedule would and errors instead.
    pub fn trigger_webhook(
        self: &Arc<Self>,
        automation_id: Uuid,
        key: &str,
    ) -> anyhow::Result<Option<AutomationRun>> {
        let automation = {
            let document = self.document();
            document
                .automations
                .iter()
                .find(|automation| automation.id == automation_id)
                .cloned()
        };
        let Some(automation) = automation else {
            return Ok(None);
        };
        let armed = automation
            .webhook_secret
            .as_deref()
            .is_some_and(|secret| secret.as_bytes().ct_eq(key.as_bytes()).into());
        if !armed {
            return Ok(None);
        }
        if !automation.enabled {
            self.record_refusal(
                &automation,
                unix_time(),
                AutomationTrigger::Webhook,
                AutomationRunStatus::SkippedUnavailable,
                "unavailable:disabled",
                Some("the automation is disabled".to_owned()),
            )?;
            bail!("automation {automation_id} is disabled");
        }
        let run = self.record_run(&automation, unix_time(), AutomationTrigger::Webhook)?;
        self.spawn_dispatch(automation, run.id)?;
        Ok(Some(run))
    }

    /// Begin the scheduler: reconcile runs a previous daemon left open,
    /// spawn the tick thread, and evaluate once so a restart or long sleep
    /// catches up immediately rather than waiting a full interval.
    pub fn start(self: &Arc<Self>, backend: &Arc<WakuBackend>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        *self.backend.lock() = Arc::downgrade(backend);
        if let Err(error) = self.reconcile_interrupted_runs() {
            eprintln!("goddard-daemon automation dispatch inhibited: {error:#}");
        }
        let service = Arc::downgrade(self);
        let stop = self.stop.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("goddard-automations".into())
            .spawn(move || {
                let (lock, condvar) = &*stop;
                loop {
                    {
                        let stopped = lock.lock().unwrap_or_else(|error| error.into_inner());
                        if *stopped {
                            return;
                        }
                        // Wakes early only for stop(); the tick itself is the
                        // timeout so evaluation cadence survives scheduling
                        // drift and laptop sleeps.
                        let guard = condvar
                            .wait_timeout(stopped, TICK_INTERVAL)
                            .unwrap_or_else(|error| error.into_inner())
                            .0;
                        if *guard {
                            return;
                        }
                    }
                    let Some(service) = service.upgrade() else {
                        return;
                    };
                    service.evaluate(Utc::now());
                }
            })
        {
            self.started.store(false, Ordering::SeqCst);
            eprintln!("goddard-daemon could not start automation scheduler: {error}");
            return;
        }
        // Catch-up evaluation after the thread exists — runs missed while
        // the daemon was down resolve inside one tick.
        self.evaluate(Utc::now());
    }

    /// Stop the tick thread. Dispatch bookkeeping keeps working — clients
    /// may still edit definitions during shutdown.
    pub fn stop(&self) {
        let (lock, condvar) = &*self.stop;
        *lock.lock().unwrap_or_else(|error| error.into_inner()) = true;
        condvar.notify_all();
    }

    /// One scheduler pass. `pub(crate)` so tests drive it with fabricated
    /// times instead of sleeping.
    pub(crate) fn evaluate(&self, now: DateTime<Utc>) {
        if self.evaluating.swap(true, Ordering::SeqCst) {
            return;
        }
        let _guard = EvaluationGuard {
            evaluating: &self.evaluating,
        };
        if let Err(error) = self.reconcile_interrupted_runs() {
            eprintln!("goddard-daemon automation dispatch inhibited: {error:#}");
            return;
        }
        let now_s = now.timestamp().max(0) as u64;
        let due: Vec<Automation> = {
            let document = self.document();
            document
                .automations
                .iter()
                .filter(|automation| automation.enabled && automation.schedule.is_some())
                .cloned()
                .collect()
        };
        for automation in due {
            if let Err(error) = self.evaluate_one(&automation, now_s) {
                eprintln!(
                    "goddard-daemon could not evaluate automation {} ({}): {error:#}",
                    automation.id, automation.name
                );
            }
        }
        self.fail_stale_runs(now_s);
        self.refresh_next_run_at(now);
    }

    /// Evaluate one enabled automation against `now`: dispatch the latest
    /// due occurrence, or record why it did not run. At most one occurrence
    /// is considered per pass — occurrences that went by earlier collapse
    /// into the newest one the way Orca's scheduler resolves them.
    fn evaluate_one(&self, automation: &Automation, now_s: u64) -> anyhow::Result<()> {
        let Some(schedule) = &automation.schedule else {
            return Ok(());
        };
        let compiled = compile_schedule(schedule)?;
        let zone = resolve_zone(&automation.timezone)?;
        let Some(occurrence) = latest_occurrence(
            &compiled,
            &zone,
            DateTime::from_timestamp(i64::try_from(now_s).context("invalid scheduler time")?, 0)
                .context("invalid scheduler time")?,
        ) else {
            return Ok(());
        };
        if occurrence < automation.created_at || occurrence > now_s {
            return Ok(());
        }
        {
            let document = self.document();
            if document.runs.iter().any(|run| {
                run.automation_id == automation.id
                    && run.trigger == AutomationTrigger::Scheduled
                    && run.scheduled_for >= occurrence
            }) {
                // Already recorded — a repeat tick must never double-fire.
                return Ok(());
            }
        }
        let grace_seconds = automation
            .missed_run_grace_minutes
            .map(|minutes| u64::from(minutes) * 60);
        match grace_seconds {
            Some(grace) if now_s.saturating_sub(occurrence) <= grace => {
                let run = self.record_run(automation, occurrence, AutomationTrigger::Scheduled)?;
                self.dispatch(automation, run.id);
            }
            _ => {
                let missed_by = format_missed_duration(now_s - occurrence);
                self.record_refusal(
                    automation,
                    occurrence,
                    AutomationTrigger::Scheduled,
                    AutomationRunStatus::SkippedMissed,
                    "missed",
                    Some(format!("scheduled for {missed_by} ago")),
                )?;
            }
        }
        Ok(())
    }

    /// Record a `Pending` run for `occurrence`, then dispatch it. The run
    /// row exists before dispatch so a crash mid-dispatch leaves an
    /// auditable record rather than a silent hole.
    fn record_run(
        &self,
        automation: &Automation,
        occurrence: u64,
        trigger: AutomationTrigger,
    ) -> anyhow::Result<AutomationRun> {
        let now = unix_time();
        let run = AutomationRun {
            id: Uuid::new_v4(),
            automation_id: automation.id,
            trigger,
            status: AutomationRunStatus::Pending,
            scheduled_for: occurrence,
            started_at: None,
            finished_at: None,
            session_id: None,
            message_id: None,
            turn_id: None,
            error: None,
            precheck: None,
            refusal_key: None,
            refusal_count: 0,
            created_at: now,
            updated_at: now,
        };
        self.transition(|document| {
            if !document
                .automations
                .iter()
                .any(|entry| entry.id == automation.id)
            {
                bail!("automation {} was removed before acceptance", automation.id);
            }
            if trigger == AutomationTrigger::Scheduled
                && document.runs.iter().any(|previous| {
                    previous.automation_id == automation.id
                        && previous.trigger == trigger
                        && previous.scheduled_for >= occurrence
                })
            {
                bail!("automation occurrence already recorded");
            }
            document.runs.insert(0, run.clone());
            if let Some(entry) = document
                .automations
                .iter_mut()
                .find(|entry| entry.id == automation.id)
            {
                entry.last_run_at = Some(now);
                entry.last_run_status = Some(AutomationRunStatus::Pending);
                entry.last_refusal_key = None;
                entry.updated_at = now;
            }
            Self::trim_runs(document, automation.id);
            Ok(run)
        })
    }

    /// Each refused occurrence keeps its own identity. Legacy folded rows
    /// stay intact, but new refusals never erase deduplication evidence.
    fn record_refusal(
        &self,
        automation: &Automation,
        occurrence: u64,
        trigger: AutomationTrigger,
        status: AutomationRunStatus,
        refusal_key: &str,
        error: Option<String>,
    ) -> anyhow::Result<()> {
        let now = unix_time();
        self.transition(|document| {
            if !document
                .automations
                .iter()
                .any(|entry| entry.id == automation.id)
            {
                bail!("automation {} was removed before acceptance", automation.id);
            }
            if trigger == AutomationTrigger::Scheduled
                && document.runs.iter().any(|run| {
                    run.automation_id == automation.id
                        && run.trigger == trigger
                        && run.scheduled_for >= occurrence
                })
            {
                return Ok(());
            }
            document.runs.insert(
                0,
                AutomationRun {
                    id: Uuid::new_v4(),
                    automation_id: automation.id,
                    trigger,
                    status,
                    scheduled_for: occurrence,
                    started_at: Some(now),
                    finished_at: Some(now),
                    session_id: None,
                    message_id: None,
                    turn_id: None,
                    error,
                    precheck: None,
                    refusal_key: Some(refusal_key.into()),
                    refusal_count: 0,
                    created_at: now,
                    updated_at: now,
                },
            );
            if let Some(entry) = document
                .automations
                .iter_mut()
                .find(|entry| entry.id == automation.id)
            {
                entry.last_run_at = Some(now);
                entry.last_run_status = Some(status);
                entry.last_refusal_key = Some(refusal_key.into());
                entry.updated_at = now;
            }
            Self::trim_runs(document, automation.id);
            Ok(())
        })
    }

    /// Dispatch on a dedicated thread — a precheck or worktree creation can
    /// block, and neither the RPC handler nor the tick loop should stall on
    /// it.
    fn spawn_dispatch(
        self: &Arc<Self>,
        automation: Automation,
        run_id: Uuid,
    ) -> anyhow::Result<()> {
        let service = self.clone();
        let spawn = || {
            std::thread::Builder::new()
                .name(format!("goddard-automation-{}", automation.id))
                .spawn(move || service.dispatch(&automation, run_id))
        };
        #[cfg(test)]
        let result = if self.fail_worker.load(Ordering::Relaxed) {
            Err(io::Error::other("injected dispatch worker failure"))
        } else {
            spawn()
        };
        #[cfg(not(test))]
        let result = spawn();
        if let Err(error) = result {
            self.finish_run(
                run_id,
                AutomationRunStatus::Failed,
                Some("the automation dispatch worker could not start".into()),
                None,
                "dispatch:worker-failed",
            );
            return Err(error).context(format!("could not start automation run {run_id}"));
        }
        Ok(())
    }

    /// Claim once in this process before any provider/precheck work. A crash
    /// cannot resurrect the claim: reopening reconciles the durable pending
    /// record to failed before any new acceptance or dispatch.
    fn claim_dispatch(&self, run_id: Uuid) -> anyhow::Result<bool> {
        let mut lifecycle = self.lifecycle.lock();
        self.ready(&mut lifecycle)?;
        if lifecycle
            .committed
            .runs
            .iter()
            .any(|run| run.id == run_id && run.status == AutomationRunStatus::Pending)
        {
            return Ok(lifecycle.dispatched.insert(run_id));
        }
        Ok(false)
    }

    /// Run precheck + dispatch for a recorded run. Called synchronously from
    /// `evaluate_one` (the `evaluating` flag already serializes evaluations)
    /// and from `run_now`'s worker.
    fn dispatch(&self, automation: &Automation, run_id: Uuid) {
        match self.claim_dispatch(run_id) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                eprintln!("goddard-daemon automation run {run_id} dispatch inhibited: {error:#}");
                self.finish_run(
                    run_id,
                    AutomationRunStatus::Failed,
                    Some("dispatch inhibited by an automation persistence failure".into()),
                    None,
                    "dispatch:persistence",
                );
                return;
            }
        }
        #[cfg(test)]
        if let Some(dispatch) = self.dispatch_hook.lock().clone() {
            dispatch(run_id);
            return;
        }
        let Some(backend) = self.backend.lock().upgrade() else {
            self.finish_run(
                run_id,
                AutomationRunStatus::SkippedUnavailable,
                Some("the daemon is shutting down".to_owned()),
                None,
                "unavailable:shutdown",
            );
            return;
        };
        if let Some(precheck) = &automation.precheck {
            let result = run_precheck(precheck);
            if !result.passed {
                let key = format!("precheck:{}", result.exit_code.unwrap_or(-1));
                self.finish_run(
                    run_id,
                    AutomationRunStatus::SkippedPrecheck,
                    result.detail.clone(),
                    Some(result),
                    &key,
                );
                return;
            }
        }
        let events = self
            .event_source
            .lock()
            .clone()
            .unwrap_or_else(EventSink::detached);
        match automation.workspace {
            AutomationWorkspace::Existing => {
                let Some(target) = automation.session_id else {
                    self.finish_run(
                        run_id,
                        AutomationRunStatus::SkippedUnavailable,
                        Some("the automation has no target task".to_owned()),
                        None,
                        "unavailable:no-target",
                    );
                    return;
                };
                self.dispatch_existing(&backend, automation, run_id, target, &events);
            }
            AutomationWorkspace::Local | AutomationWorkspace::Worktree => {
                if automation.reuse_session {
                    if let Some(target) = automation
                        .last_session_id
                        .filter(|target| backend.known_session(*target))
                    {
                        self.dispatch_existing(&backend, automation, run_id, target, &events);
                        return;
                    }
                }
                let workspace = match automation.workspace {
                    AutomationWorkspace::Local => AgentWorkspace::Local,
                    AutomationWorkspace::Worktree => AgentWorkspace::Worktree,
                    AutomationWorkspace::Existing => unreachable!(),
                };
                let result = backend.create_automation_task(
                    run_id,
                    AgentCreateSelection {
                        provider: Some(automation.provider),
                        model: automation.model.clone(),
                        title: None,
                        reasoning_effort: None,
                        service_tier: None,
                        context_window: None,
                    },
                    automation.project_path.clone(),
                    workspace,
                    automation.base_branch.clone(),
                    automation.prompt.clone(),
                    &events,
                );
                // A failed launch can still have persisted the session — bump
                // either way so clients reload and the task appears.
                self.notify_tasks_changed();
                match result {
                    Ok(_) => {}
                    Err(error) => self.finish_run(
                        run_id,
                        AutomationRunStatus::Failed,
                        Some(format!("{error:#}")),
                        None,
                        "dispatch:failed",
                    ),
                }
            }
        }
    }

    /// Deliver the prompt to an existing task through the queue path — the
    /// same one `agent prompt` uses — so it lands when the task goes idle.
    fn dispatch_existing(
        &self,
        backend: &Arc<WakuBackend>,
        automation: &Automation,
        run_id: Uuid,
        target: Uuid,
        events: &EventSink,
    ) {
        if !backend.known_session(target) {
            self.finish_run(
                run_id,
                AutomationRunStatus::SkippedUnavailable,
                Some("the target task no longer exists".to_owned()),
                None,
                "unavailable:missing-task",
            );
            return;
        }
        if backend.session_quarantined(target) {
            self.finish_run(
                run_id,
                AutomationRunStatus::SkippedUnavailable,
                Some("the target task is quarantined".to_owned()),
                None,
                "unavailable:quarantined",
            );
            return;
        }
        let message_id = Uuid::new_v4();
        if let Err(error) = self.bind_message(run_id, target, message_id) {
            eprintln!("goddard-daemon could not bind automation run {run_id}: {error:#}");
            self.finish_run(run_id, AutomationRunStatus::Failed,
                Some("the automation message association could not be saved; no prompt was dispatched".into()),
                None, "dispatch:binding-failed");
            return;
        }
        match backend.queue_agent_prompt_with_id(
            target,
            automation.prompt.clone(),
            None,
            false,
            Some(message_id),
            None,
            events,
        ) {
            Ok(()) => self.notify_tasks_changed(),
            Err(error) => self.finish_run(
                run_id,
                AutomationRunStatus::Failed,
                Some(format!("{error:#}")),
                None,
                "dispatch:failed",
            ),
        }
    }

    /// C's stable association is durably installed before queueing or
    /// creating a provider turn. A failed binding must not send the prompt.
    pub(crate) fn bind_message(
        &self,
        run_id: Uuid,
        session_id: Uuid,
        message_id: Uuid,
    ) -> anyhow::Result<()> {
        self.transition(|document| {
            let run = document
                .runs
                .iter_mut()
                .find(|run| run.id == run_id)
                .ok_or_else(|| anyhow!("automation run {run_id} is missing"))?;
            anyhow::ensure!(
                run.status == AutomationRunStatus::Pending,
                "automation run is no longer pending"
            );
            anyhow::ensure!(
                run.message_id.is_none()
                    || (run.message_id == Some(message_id) && run.session_id == Some(session_id)),
                "automation run already belongs to another message"
            );
            run.session_id = Some(session_id);
            run.message_id = Some(message_id);
            run.updated_at = unix_time();
            Ok(())
        })
    }

    pub(crate) fn has_open_run(&self, session_id: Uuid) -> bool {
        let lifecycle = self.lifecycle.lock();
        lifecycle
            .repair
            .as_ref()
            .unwrap_or(&lifecycle.committed)
            .runs
            .iter()
            .any(|run| run.session_id == Some(session_id) && !run.status.is_terminal())
    }

    pub(crate) fn is_run_message(&self, message_id: Uuid) -> bool {
        let lifecycle = self.lifecycle.lock();
        lifecycle
            .repair
            .as_ref()
            .unwrap_or(&lifecycle.committed)
            .runs
            .iter()
            .any(|run| run.message_id == Some(message_id))
    }

    /// C calls this before the provider send. Failure discards Running and
    /// prevents send; the caller resolves the already-accepted run as failed.
    pub(crate) fn note_prompt_delivered(
        &self,
        session_id: Uuid,
        message_id: Uuid,
        turn_id: Uuid,
    ) -> anyhow::Result<()> {
        let now = unix_time();
        self.transition(|document| {
            let Some(run) = document.runs.iter_mut().find(|run| {
                run.session_id == Some(session_id) && run.message_id == Some(message_id)
            }) else {
                return Ok(());
            };
            anyhow::ensure!(
                run.status == AutomationRunStatus::Pending,
                "automation run is no longer pending"
            );
            run.status = AutomationRunStatus::Running;
            run.turn_id = Some(turn_id);
            run.started_at = Some(now);
            run.updated_at = now;
            let automation_id = run.automation_id;
            if let Some(automation) = document
                .automations
                .iter_mut()
                .find(|entry| entry.id == automation_id)
            {
                automation.last_session_id = Some(session_id);
                if document
                    .runs
                    .iter()
                    .find(|run| run.automation_id == automation_id)
                    .is_some_and(|run| {
                        run.session_id == Some(session_id) && run.message_id == Some(message_id)
                    })
                {
                    automation.last_run_status = Some(AutomationRunStatus::Running);
                    automation.last_refusal_key = None;
                    automation.updated_at = now;
                }
            }
            Ok(())
        })
    }

    /// First terminal verdict wins, even if its save failed. Retrying a
    /// write or receiving a duplicate event cannot overwrite that verdict.
    fn settle(
        document: &mut AutomationsState,
        run_id: Uuid,
        status: AutomationRunStatus,
        error: Option<String>,
        precheck: Option<AutomationPrecheckResult>,
        refusal_key: &str,
        now: u64,
    ) {
        let Some(run) = document
            .runs
            .iter_mut()
            .find(|run| run.id == run_id && !run.status.is_terminal())
        else {
            return;
        };
        let automation_id = run.automation_id;
        run.status = status;
        run.error = error;
        run.precheck = precheck;
        run.finished_at = Some(now);
        run.refusal_key = (!refusal_key.is_empty()).then(|| refusal_key.to_owned());
        run.updated_at = now;
        if document
            .runs
            .iter()
            .find(|run| run.automation_id == automation_id)
            .is_some_and(|run| run.id == run_id)
        {
            if let Some(automation) = document
                .automations
                .iter_mut()
                .find(|entry| entry.id == automation_id)
            {
                automation.last_run_status = Some(status);
                automation.last_refusal_key =
                    (!refusal_key.is_empty()).then(|| refusal_key.to_owned());
                automation.updated_at = now;
            }
        }
        // Retirement happens on later acceptance, never in the same write
        // as this run's first terminal verdict.
    }

    fn finish_run(
        &self,
        run_id: Uuid,
        status: AutomationRunStatus,
        error: Option<String>,
        precheck: Option<AutomationPrecheckResult>,
        refusal_key: &str,
    ) {
        self.report_bookkeeping(
            run_id,
            self.bookkeeping(|document| {
                Self::settle(
                    document,
                    run_id,
                    status,
                    error,
                    precheck,
                    refusal_key,
                    unix_time(),
                );
            }),
        );
    }

    /// C selects the attributed turn; B durably fixes the first verdict.
    pub fn note_driver_event(&self, session_id: Uuid, turn_id: Option<Uuid>, event: &DriverEvent) {
        let (status, error) = match event {
            DriverEvent::TurnFinished { success: true, .. } => {
                (AutomationRunStatus::Completed, None)
            }
            DriverEvent::TurnFinished {
                success: false,
                summary,
                ..
            } => (
                AutomationRunStatus::Failed,
                Some(
                    summary
                        .clone()
                        .unwrap_or_else(|| "the turn did not complete".to_owned()),
                ),
            ),
            DriverEvent::ProcessExited => (
                AutomationRunStatus::Failed,
                Some("the provider process exited".to_owned()),
            ),
            _ => return,
        };
        let mut affected_run = None;
        let result = self.bookkeeping(|document| {
            if let Some(run_id) = document
                .runs
                .iter()
                .find(|run| {
                    run.session_id == Some(session_id)
                        && run.status == AutomationRunStatus::Running
                        && turn_id.is_some()
                        && run.turn_id == turn_id
                })
                .map(|run| run.id)
            {
                affected_run = Some(run_id);
                Self::settle(document, run_id, status, error, None, "", unix_time());
            }
        });
        if let Some(run_id) = affected_run {
            self.report_bookkeeping(run_id, result);
        } else if let Err(error) = result {
            eprintln!(
                "goddard-daemon automation bookkeeping for session {session_id} uncertain: {error:#}"
            );
        }
    }

    /// Retries bookkeeping only. Failed reconciliation inhibits every
    /// acceptance/dispatch entry point, including manual and webhook calls.
    fn reconcile_interrupted_runs(&self) -> anyhow::Result<()> {
        self.ready(&mut self.lifecycle.lock())
    }

    fn fail_stale_runs(&self, now_s: u64) {
        let result = self.bookkeeping(|document| {
            let stale: Vec<_> = document
                .runs
                .iter()
                .filter(|run| {
                    run.status == AutomationRunStatus::Running
                        && run.started_at.is_some_and(|started| {
                            now_s.saturating_sub(started) > STALE_RUN_TIMEOUT_SECONDS
                        })
                })
                .map(|run| run.id)
                .collect();
            for run_id in stale {
                Self::settle(
                    document,
                    run_id,
                    AutomationRunStatus::Failed,
                    Some("the run timed out waiting for the task".into()),
                    None,
                    "timeout",
                    now_s,
                );
            }
        });
        if let Err(error) = result {
            eprintln!("goddard-daemon automation timeout bookkeeping uncertain: {error:#}");
        }
    }

    fn refresh_next_run_at(&self, now: DateTime<Utc>) {
        let result = self.transition(|document| {
            for automation in &mut document.automations {
                automation.next_run_at = automation
                    .enabled
                    .then(|| {
                        automation.schedule.as_ref().and_then(|schedule| {
                            compile_schedule(schedule).ok().and_then(|compiled| {
                                resolve_zone(&automation.timezone)
                                    .ok()
                                    .and_then(|zone| next_occurrence(&compiled, &zone, now))
                            })
                        })
                    })
                    .flatten();
            }
            Ok(())
        });
        if let Err(error) = result {
            eprintln!("goddard-daemon could not persist automation schedule metadata: {error:#}");
        }
    }

    /// Bound terminal history, retain all in-flight runs and the newest
    /// scheduled identity as a retirement watermark. Older occurrences can
    /// never dispatch again after history trimming or restart.
    fn trim_runs(document: &mut AutomationsState, automation_id: Uuid) {
        let watermark = document
            .runs
            .iter()
            .filter(|run| {
                run.automation_id == automation_id && run.trigger == AutomationTrigger::Scheduled
            })
            .max_by_key(|run| run.scheduled_for)
            .map(|run| run.id);
        let mut kept = 0usize;
        document.runs.retain(|run| {
            if run.automation_id != automation_id || !run.status.is_terminal() {
                return true;
            }
            if Some(run.id) == watermark {
                return true;
            }
            kept += 1;
            kept <= MAX_RUNS_PER_AUTOMATION - usize::from(watermark.is_some())
        });
    }
}

/// Drop guard restoring `evaluating` on every exit path, panic included.
struct EvaluationGuard<'a> {
    evaluating: &'a AtomicBool,
}

impl Drop for EvaluationGuard<'_> {
    fn drop(&mut self) {
        self.evaluating.store(false, Ordering::SeqCst);
    }
}

/// The key in a webhook URL — 32 hex chars, ~122 bits, enough that the URL
/// alone is the credential.
fn new_webhook_secret() -> String {
    Uuid::new_v4().simple().to_string()
}

/// Field validation shared by create and edit: reject a definition that
/// could never run.
fn validate_input(input: &AutomationInput) -> anyhow::Result<()> {
    if input.name.trim().is_empty() {
        bail!("automations require a name");
    }
    if input.prompt.trim().is_empty() {
        bail!("automations require a prompt");
    }
    if !input.project_path.is_absolute() {
        bail!("the project path must be absolute");
    }
    if matches!(input.workspace, AutomationWorkspace::Existing) && input.session_id.is_none() {
        bail!("an existing-task automation requires a target task");
    }
    if matches!(input.workspace, AutomationWorkspace::Worktree)
        && input
            .base_branch
            .as_deref()
            .is_none_or(|branch| branch.trim().is_empty())
    {
        bail!("worktree automations require a base branch");
    }
    if input.enabled && input.schedule.is_none() && !input.webhook {
        bail!("an enabled automation requires a schedule or a webhook");
    }
    if let Some(schedule) = &input.schedule {
        let compiled = compile_schedule(schedule)?;
        let zone = resolve_zone(&input.timezone)?;
        if next_occurrence(&compiled, &zone, Utc::now()).is_none() {
            bail!("the schedule has no upcoming occurrences");
        }
    }
    Ok(())
}

/// Compile a schedule preset or custom expression into a `cron::Schedule`.
/// Presets become six-field expressions (seconds first); custom input
/// accepts standard five-field cron, or six/seven fields verbatim.
fn compile_schedule(schedule: &AutomationSchedule) -> anyhow::Result<Schedule> {
    let expression = match schedule {
        AutomationSchedule::Hourly { minute } if *minute < 60 => {
            format!("0 {minute} * * * *")
        }
        AutomationSchedule::Daily { hour, minute } if *hour < 24 && *minute < 60 => {
            format!("0 {minute} {hour} * * *")
        }
        // The cron crate numbers days 1=Sunday..7=Saturday and rejects 0;
        // the protocol keeps the more common 0=Sunday..6=Saturday.
        AutomationSchedule::Weekdays { hour, minute } if *hour < 24 && *minute < 60 => {
            format!("0 {minute} {hour} * * 2-6")
        }
        AutomationSchedule::Weekly {
            day_of_week,
            hour,
            minute,
        } if *day_of_week < 7 && *hour < 24 && *minute < 60 => {
            format!("0 {minute} {hour} * * {}", day_of_week + 1)
        }
        AutomationSchedule::Cron { expression } => {
            let expression = expression.trim();
            match expression.split_whitespace().count() {
                5 => format!("0 {expression}"),
                6 | 7 => expression.to_owned(),
                _ => bail!("cron expressions take five to seven fields"),
            }
        }
        _ => bail!("schedule fields are out of range"),
    };
    Schedule::from_str(&expression)
        .with_context(|| format!("could not parse schedule {expression:?}"))
}

/// The zone schedule math runs in: the daemon's local zone, or a named
/// IANA zone for automations pinned to one.
enum AutomationZone {
    Local,
    Named(chrono_tz::Tz),
}

fn resolve_zone(timezone: &Option<String>) -> anyhow::Result<AutomationZone> {
    match timezone.as_deref().map(str::trim) {
        None | Some("") => Ok(AutomationZone::Local),
        Some(name) => chrono_tz::Tz::from_str(name)
            .map(AutomationZone::Named)
            .with_context(|| format!("unknown timezone {name:?}")),
    }
}

/// The most recent occurrence at or before `now`, or `None` when the
/// schedule has none (an impossible cron like February 31st).
fn latest_occurrence(
    schedule: &Schedule,
    zone: &AutomationZone,
    now: DateTime<Utc>,
) -> Option<u64> {
    match zone {
        AutomationZone::Local => latest_occurrence_in(schedule, &now.with_timezone(&chrono::Local)),
        AutomationZone::Named(zone) => latest_occurrence_in(schedule, &now.with_timezone(zone)),
    }
}

/// `cron` iterates forward only, so "latest at-or-before" walks from a
/// lookback anchor and keeps the last occurrence not past `now`. A short
/// window first — dense schedules hit their fire within a few thousand
/// steps — and when that comes up empty the schedule is sparse enough
/// that a year of occurrences is still a small walk.
fn latest_occurrence_in<Z: TimeZone>(schedule: &Schedule, now: &DateTime<Z>) -> Option<u64> {
    for lookback in [chrono::Duration::days(3), chrono::Duration::days(400)] {
        let mut latest = None;
        for occurrence in schedule.after(&(now.clone() - lookback)) {
            if occurrence > *now {
                break;
            }
            latest = Some(occurrence.timestamp().max(0) as u64);
        }
        if latest.is_some() {
            return latest;
        }
    }
    None
}

/// The first occurrence strictly after `after`.
fn next_occurrence(
    schedule: &Schedule,
    zone: &AutomationZone,
    after: DateTime<Utc>,
) -> Option<u64> {
    match zone {
        AutomationZone::Local => next_occurrence_in(schedule, &after.with_timezone(&chrono::Local)),
        AutomationZone::Named(zone) => next_occurrence_in(schedule, &after.with_timezone(zone)),
    }
}

fn next_occurrence_in<Z: TimeZone>(schedule: &Schedule, after: &DateTime<Z>) -> Option<u64> {
    schedule
        .after(after)
        .next()
        .map(|occurrence| occurrence.timestamp().max(0) as u64)
}

/// Run the precheck command through the platform shell with a hard timeout.
/// Output is capped — the detail only needs enough tail to explain a skip.
fn run_precheck(precheck: &AutomationPrecheck) -> AutomationPrecheckResult {
    let mut command = crate::command_env::plain_command(shell_program());
    command.args(shell_args(&precheck.command));
    command.current_dir("/");
    let timeout = Duration::from_secs(precheck.timeout_seconds.max(1));
    let mut child = match command.spawn_bounded() {
        Ok(child) => child,
        Err(error) => {
            return AutomationPrecheckResult {
                passed: false,
                exit_code: None,
                detail: Some(format!("precheck could not start: {error}")),
            };
        }
    };
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().ok();
                let status = output.as_ref().map(|output| output.status);
                let exit_code = status.and_then(|status| status.code());
                let passed = status.is_some_and(|status| status.success());
                let detail = output
                    .as_ref()
                    .map(|output| tail_output(output))
                    .filter(|detail| !detail.is_empty());
                return AutomationPrecheckResult {
                    passed,
                    exit_code,
                    detail: if passed { None } else { detail },
                };
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return AutomationPrecheckResult {
                        passed: false,
                        exit_code: None,
                        detail: Some(format!(
                            "precheck timed out after {}s",
                            precheck.timeout_seconds
                        )),
                    };
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                return AutomationPrecheckResult {
                    passed: false,
                    exit_code: None,
                    detail: Some(format!("precheck could not be waited on: {error}")),
                };
            }
        }
    }
}

#[cfg(unix)]
fn shell_program() -> &'static str {
    "/bin/sh"
}

#[cfg(unix)]
fn shell_args(command: &str) -> [&str; 2] {
    ["-c", command]
}

#[cfg(windows)]
fn shell_program() -> &'static str {
    "cmd.exe"
}

#[cfg(windows)]
fn shell_args(command: &str) -> [&str; 2] {
    ["/C", command]
}

/// Last ~500 bytes of combined output — the interesting part of a failed
/// check is almost always its tail.
fn tail_output(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim());
    }
    let trimmed = text.trim();
    if trimmed.len() > 500 {
        trimmed[trimmed.len() - 500..].to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn format_missed_duration(seconds: u64) -> String {
    if seconds >= 86_400 {
        format!("{} day(s)", seconds / 86_400)
    } else if seconds >= 3_600 {
        format!("{} hour(s)", seconds / 3_600)
    } else {
        format!("{} minute(s)", (seconds / 60).max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use waku_protocol::model::ProviderKind;

    fn daily_at(hour: u8, minute: u8) -> AutomationSchedule {
        AutomationSchedule::Daily { hour, minute }
    }

    fn service() -> Arc<AutomationService> {
        let directory =
            std::env::temp_dir().join(format!("waku-automations-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        Arc::new(AutomationService::open(directory.join("automations.json")).unwrap())
    }

    fn input(name: &str) -> AutomationInput {
        AutomationInput {
            id: None,
            name: name.to_owned(),
            prompt: "do the thing".to_owned(),
            provider: ProviderKind::Claude,
            model: None,
            project_path: std::env::temp_dir(),
            workspace: AutomationWorkspace::Local,
            base_branch: None,
            session_id: None,
            schedule: Some(daily_at(9, 0)),
            webhook: false,
            timezone: None,
            enabled: true,
            precheck: None,
            missed_run_grace_minutes: None,
            reuse_session: false,
        }
    }

    fn compile(schedule: &AutomationSchedule) -> Schedule {
        compile_schedule(schedule).expect("schedule should compile")
    }

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, s).unwrap()
    }

    #[test]
    fn presets_compile_to_cron() {
        assert_eq!(
            compile_schedule(&AutomationSchedule::Hourly { minute: 15 })
                .unwrap()
                .to_string(),
            "0 15 * * * *"
        );
        assert!(compile_schedule(&AutomationSchedule::Hourly { minute: 60 }).is_err());
        assert!(compile_schedule(&daily_at(24, 0)).is_err());
        assert!(
            compile_schedule(&AutomationSchedule::Weekly {
                day_of_week: 7,
                hour: 9,
                minute: 0
            })
            .is_err()
        );
    }

    #[test]
    fn custom_cron_accepts_five_fields() {
        let schedule = compile(&AutomationSchedule::Cron {
            expression: "*/5 * * * *".to_owned(),
        });
        // 2024-01-01 00:00 UTC is a Monday.
        let after = utc(2024, 1, 1, 0, 3, 0);
        assert_eq!(
            next_occurrence(&schedule, &AutomationZone::Named(chrono_tz::UTC), after),
            Some(utc(2024, 1, 1, 0, 5, 0).timestamp() as u64)
        );
    }

    #[test]
    fn custom_cron_rejects_bad_field_counts() {
        for expression in ["* * * *", "* * * * * * * *", "not a schedule"] {
            assert!(
                compile_schedule(&AutomationSchedule::Cron {
                    expression: expression.to_owned()
                })
                .is_err(),
                "{expression} should not compile"
            );
        }
    }

    #[test]
    fn latest_occurrence_includes_now() {
        let schedule = compile(&daily_at(9, 0));
        let zone = AutomationZone::Named(chrono_tz::UTC);
        let exactly = utc(2024, 1, 2, 9, 0, 0);
        assert_eq!(
            latest_occurrence(&schedule, &zone, exactly),
            Some(exactly.timestamp() as u64)
        );
    }

    #[test]
    fn latest_occurrence_is_the_previous_fire() {
        let schedule = compile(&daily_at(9, 0));
        let zone = AutomationZone::Named(chrono_tz::UTC);
        let now = utc(2024, 1, 2, 15, 30, 0);
        assert_eq!(
            latest_occurrence(&schedule, &zone, now),
            Some(utc(2024, 1, 2, 9, 0, 0).timestamp() as u64)
        );
    }

    #[test]
    fn weekdays_skip_weekends() {
        let schedule = compile(&AutomationSchedule::Weekdays { hour: 9, minute: 0 });
        let zone = AutomationZone::Named(chrono_tz::UTC);
        // 2024-01-06 is a Saturday; the latest weekday occurrence is Friday.
        let saturday = utc(2024, 1, 6, 12, 0, 0);
        assert_eq!(
            latest_occurrence(&schedule, &zone, saturday),
            Some(utc(2024, 1, 5, 9, 0, 0).timestamp() as u64)
        );
    }

    #[test]
    fn timezone_shifts_the_fire_instant() {
        let schedule = compile(&daily_at(9, 0));
        let zone = AutomationZone::Named(chrono_tz::Tz::America__Los_Angeles);
        // 9:00 PST = 17:00 UTC in January.
        let now = utc(2024, 1, 2, 17, 30, 0);
        assert_eq!(
            latest_occurrence(&schedule, &zone, now),
            Some(utc(2024, 1, 2, 17, 0, 0).timestamp() as u64)
        );
    }

    #[test]
    fn unknown_timezone_is_rejected() {
        assert!(resolve_zone(&Some("Mars/Olympus".to_owned())).is_err());
        assert!(matches!(
            resolve_zone(&None).unwrap(),
            AutomationZone::Local
        ));
        assert!(matches!(
            resolve_zone(&Some("   ".to_owned())).unwrap(),
            AutomationZone::Local
        ));
    }

    #[test]
    fn next_occurrence_is_strictly_after() {
        let schedule = compile(&daily_at(9, 0));
        let zone = AutomationZone::Named(chrono_tz::UTC);
        let exactly = utc(2024, 1, 2, 9, 0, 0);
        assert_eq!(
            next_occurrence(&schedule, &zone, exactly),
            Some(utc(2024, 1, 3, 9, 0, 0).timestamp() as u64)
        );
    }

    #[test]
    fn enabled_automation_needs_a_schedule_or_webhook() {
        let service = service();
        let mut input = input("bare");
        input.schedule = None;
        assert!(service.upsert(input.clone()).is_err());
        input.webhook = true;
        assert!(service.upsert(input).is_ok());
    }

    #[test]
    fn upsert_mints_preserves_and_disarms_the_webhook_secret() {
        let service = service();
        let mut create = input("hooked");
        create.schedule = None;
        create.webhook = true;
        let automation = service.upsert(create).unwrap();
        let secret = automation.webhook_secret.clone().expect("webhook armed");
        assert_eq!(secret.len(), 32);

        let mut edit = input("renamed");
        edit.id = Some(automation.id);
        edit.schedule = None;
        edit.webhook = true;
        let edited = service.upsert(edit.clone()).unwrap();
        assert_eq!(edited.webhook_secret.as_deref(), Some(secret.as_str()));

        // Disarming while the automation stays enabled means switching it
        // back to a schedule — an enabled trigger-less automation is invalid.
        edit.webhook = false;
        edit.schedule = Some(daily_at(9, 0));
        let disarmed = service.upsert(edit).unwrap();
        assert_eq!(disarmed.webhook_secret, None);
    }

    #[test]
    fn webhook_trigger_checks_the_key() {
        let service = service();
        let mut create = input("hooked");
        create.schedule = None;
        create.webhook = true;
        let automation = service.upsert(create).unwrap();
        let secret = automation.webhook_secret.clone().unwrap();

        assert!(
            service
                .trigger_webhook(automation.id, "wrong")
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .trigger_webhook(Uuid::new_v4(), &secret)
                .unwrap()
                .is_none()
        );
        let run = service
            .trigger_webhook(automation.id, &secret)
            .unwrap()
            .expect("armed webhook fires");
        assert_eq!(run.trigger, AutomationTrigger::Webhook);
        assert_eq!(run.automation_id, automation.id);
    }

    #[test]
    fn webhook_on_a_disabled_automation_records_a_refusal() {
        let service = service();
        let mut create = input("hooked");
        create.schedule = None;
        create.webhook = true;
        create.enabled = false;
        let automation = service.upsert(create).unwrap();
        let secret = automation.webhook_secret.clone().unwrap();

        assert!(service.trigger_webhook(automation.id, &secret).is_err());
        let document = service.document();
        let run = document
            .runs
            .iter()
            .find(|run| run.automation_id == automation.id)
            .expect("the refusal is recorded");
        assert_eq!(run.trigger, AutomationTrigger::Webhook);
        assert_eq!(run.status, AutomationRunStatus::SkippedUnavailable);
    }
    fn reopened(service: &AutomationService) -> AutomationService {
        AutomationService::open(service.store.path.clone()).unwrap()
    }

    fn finished(success: bool) -> DriverEvent {
        DriverEvent::TurnFinished {
            success,
            summary: None,
            summary_i18n: None,
        }
    }

    #[test]
    fn run_transitions_survive_restart_without_schedule_or_definition_edits() {
        for success in [true, false] {
            let service = service();
            let mut create = input("durable");
            create.schedule = None;
            create.webhook = true;
            let automation = service.upsert(create).unwrap();
            let run = service
                .record_run(&automation, 123, AutomationTrigger::Manual)
                .unwrap();
            assert_eq!(reopened(&service).document().runs[0], run);
            let session_id = Uuid::new_v4();
            let message_id = Uuid::new_v4();
            let turn_id = Uuid::new_v4();
            service
                .bind_message(run.id, session_id, message_id)
                .unwrap();
            let queued = reopened(&service).document().runs.remove(0);
            assert_eq!(queued.status, AutomationRunStatus::Pending);
            assert_eq!(queued.message_id, Some(message_id));
            assert_eq!(queued.turn_id, None);
            service
                .note_prompt_delivered(session_id, message_id, turn_id)
                .unwrap();
            assert_eq!(
                reopened(&service).document().runs[0].status,
                AutomationRunStatus::Running
            );
            assert_eq!(service.document().runs[0].turn_id, Some(turn_id));
            service.note_driver_event(session_id, Some(turn_id), &finished(success));
            let terminal = reopened(&service);
            let expected = if success {
                AutomationRunStatus::Completed
            } else {
                AutomationRunStatus::Failed
            };
            assert_eq!(terminal.document().runs[0].status, expected);
            terminal.reconcile_interrupted_runs().unwrap();
            assert_eq!(reopened(&terminal).document().runs[0].status, expected);
            fs::remove_dir_all(service.store.path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn interrupted_pending_and_running_runs_fail_durably_at_boot() {
        for delivered in [false, true] {
            let service = service();
            let automation = service.upsert(input("interrupted")).unwrap();
            let run = service
                .record_run(&automation, 123, AutomationTrigger::Manual)
                .unwrap();
            if delivered {
                let session = Uuid::new_v4();
                let message = Uuid::new_v4();
                service.bind_message(run.id, session, message).unwrap();
                service
                    .note_prompt_delivered(session, message, Uuid::new_v4())
                    .unwrap();
            }
            let restored = reopened(&service);
            restored.reconcile_interrupted_runs().unwrap();
            let document = reopened(&restored).document();
            assert_eq!(
                document.automations[0].last_run_status,
                Some(AutomationRunStatus::Failed)
            );
            let run = document.runs[0].clone();
            assert_eq!(run.status, AutomationRunStatus::Failed);
            assert_eq!(
                run.error.as_deref(),
                Some("the daemon restarted before the run finished; work will not be replayed")
            );
            fs::remove_dir_all(service.store.path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn failed_persistence_rejects_dispatch_and_does_not_publish_a_transition() {
        use std::sync::atomic::AtomicUsize;
        let service = service();
        let automation = service.upsert(input("write failure")).unwrap();
        let published = Arc::new(AtomicUsize::new(0));
        let count = published.clone();
        service.set_sink(Arc::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
        }));
        // A directory at the atomic-write temporary path fails even as root.
        let temporary = service.store.path.with_extension("json.tmp");
        fs::create_dir(&temporary).unwrap();
        assert!(
            service
                .run_now(automation.id)
                .unwrap_err()
                .to_string()
                .contains("persist")
        );
        assert!(service.document().runs.is_empty());
        assert!(reopened(&service).document().runs.is_empty());
        assert_eq!(published.load(Ordering::SeqCst), 0);
        fs::remove_dir(&temporary).unwrap();
        let run = service
            .record_run(&automation, 123, AutomationTrigger::Manual)
            .unwrap();
        let session = Uuid::new_v4();
        let message = Uuid::new_v4();
        let turn = Uuid::new_v4();
        service.bind_message(run.id, session, message).unwrap();
        let before = service.document();
        let broadcasts = published.load(Ordering::SeqCst);
        fs::create_dir(&temporary).unwrap();
        assert!(
            service
                .note_prompt_delivered(session, message, turn)
                .is_err()
        );
        assert_eq!(service.document(), before);
        assert_eq!(published.load(Ordering::SeqCst), broadcasts);
        fs::remove_dir(&temporary).unwrap();
        service
            .note_prompt_delivered(session, message, turn)
            .unwrap();
        let before = service.document();
        let broadcasts = published.load(Ordering::SeqCst);
        fs::create_dir(&temporary).unwrap();
        service.note_driver_event(session, Some(turn), &finished(true));
        assert_eq!(service.document().runs[0].status, before.runs[0].status);
        assert!(
            service.document().runs[0]
                .error
                .as_deref()
                .unwrap()
                .contains("uncertain")
        );
        assert_eq!(reopened(&service).document(), before);
        assert_eq!(published.load(Ordering::SeqCst), broadcasts + 1);
        fs::remove_dir(&temporary).unwrap();
        service.note_driver_event(session, Some(turn), &finished(true));
        assert_eq!(
            reopened(&service).document().runs[0].status,
            AutomationRunStatus::Completed
        );
        fs::remove_dir_all(service.store.path.parent().unwrap()).unwrap();
    }

    #[test]
    fn dispatch_failures_and_stale_runs_are_persisted() {
        let service = service();
        let automation = service.upsert(input("failed dispatch")).unwrap();
        let failed = service
            .record_run(&automation, 123, AutomationTrigger::Manual)
            .unwrap();
        service.finish_run(
            failed.id,
            AutomationRunStatus::Failed,
            Some("launch failed".into()),
            None,
            "dispatch:failed",
        );
        assert_eq!(
            reopened(&service).document().runs[0].error.as_deref(),
            Some("launch failed")
        );
        let stale = service
            .record_run(&automation, 124, AutomationTrigger::Manual)
            .unwrap();
        let session = Uuid::new_v4();
        let message = Uuid::new_v4();
        service.bind_message(stale.id, session, message).unwrap();
        service
            .note_prompt_delivered(session, message, Uuid::new_v4())
            .unwrap();
        service.fail_stale_runs(unix_time() + STALE_RUN_TIMEOUT_SECONDS + 1);
        assert_eq!(
            reopened(&service).document().runs[0].status,
            AutomationRunStatus::Failed
        );
        fs::remove_dir_all(service.store.path.parent().unwrap()).unwrap();
    }
    fn begin_run(
        service: &AutomationService,
        run_id: Uuid,
        _automation_id: Uuid,
        session_id: Uuid,
    ) -> Uuid {
        let message = Uuid::new_v4();
        let turn = Uuid::new_v4();
        service.bind_message(run_id, session_id, message).unwrap();
        service
            .note_prompt_delivered(session_id, message, turn)
            .unwrap();
        turn
    }

    fn persisted(service: &AutomationService) -> AutomationsState {
        serde_json::from_slice(&fs::read(&service.store.path).unwrap()).unwrap()
    }

    fn inject_write_failure(service: &AutomationService) {
        *service.store.before_save.lock() = Some(Arc::new(|_| {
            Err(io::Error::other("injected automation write failure"))
        }));
    }

    fn allow_writes(service: &AutomationService) {
        *service.store.before_save.lock() = None;
    }

    #[test]
    fn pending_is_saved_before_each_dispatch_and_failed_acceptance_changes_nothing() {
        use std::sync::mpsc;
        let service = service();
        let mut create = input("durable");
        create.webhook = true;
        create.timezone = Some("UTC".into());
        create.missed_run_grace_minutes = Some(1);
        let mut automation = service.upsert(create).unwrap();
        service
            .transition(|document| {
                document.automations[0].created_at = 0;
                Ok(())
            })
            .unwrap();
        automation.created_at = 0;
        let (sent, received) = mpsc::channel();
        let path = service.store.path.clone();
        *service.dispatch_hook.lock() = Some(Arc::new(move |run_id| {
            let state: AutomationsState =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            let run = state.runs.iter().find(|run| run.id == run_id).unwrap();
            assert_eq!(run.status, AutomationRunStatus::Pending);
            sent.send(run_id).unwrap();
        }));
        let manual = service.run_now(automation.id).unwrap();
        assert_eq!(received.recv().unwrap(), manual.id);
        let webhook = service
            .trigger_webhook(automation.id, automation.webhook_secret.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(received.recv().unwrap(), webhook.id);
        let now = utc(2026, 10, 11, 9, 0, 0);
        service
            .evaluate_one(&automation, now.timestamp() as u64)
            .unwrap();
        let scheduled = received.recv().unwrap();
        service
            .evaluate_one(&automation, now.timestamp() as u64)
            .unwrap();
        assert!(received.try_recv().is_err());
        assert_eq!(persisted(&service).runs[0].id, scheduled);

        // Seed enough committed history that a failed pending candidate
        // would trim rows and change last-run metadata if it leaked.
        service
            .transition(|document| {
                let template = document.runs[0].clone();
                for occurrence in 1..=MAX_RUNS_PER_AUTOMATION {
                    let mut run = template.clone();
                    run.id = Uuid::new_v4();
                    run.trigger = AutomationTrigger::Manual;
                    run.status = AutomationRunStatus::Completed;
                    run.scheduled_for = occurrence as u64;
                    document.runs.push(run);
                }
                Ok(())
            })
            .unwrap();
        let before = service.document();
        let published = Arc::new(Mutex::new(Vec::new()));
        let capture = published.clone();
        service.set_sink(Arc::new(move |state| capture.lock().push(state)));
        inject_write_failure(&service);
        assert!(service.run_now(automation.id).is_err());
        assert!(
            service
                .trigger_webhook(automation.id, automation.webhook_secret.as_deref().unwrap())
                .is_err()
        );
        let next = utc(2026, 10, 12, 9, 0, 0);
        assert!(
            service
                .evaluate_one(&automation, next.timestamp() as u64)
                .is_err()
        );
        assert_eq!(service.document(), before);
        assert_eq!(persisted(&service), before);
        assert!(published.lock().is_empty());
        assert!(received.try_recv().is_err());
        allow_writes(&service);
        service
            .evaluate_one(&automation, next.timestamp() as u64)
            .unwrap();
        assert_eq!(received.recv().unwrap(), persisted(&service).runs[0].id);
    }

    #[test]
    fn every_lifecycle_verdict_survives_without_an_unrelated_write() {
        let service = service();
        let automation = service.upsert(input("history")).unwrap();
        for status in [
            AutomationRunStatus::Completed,
            AutomationRunStatus::Failed,
            AutomationRunStatus::SkippedPrecheck,
            AutomationRunStatus::SkippedUnavailable,
        ] {
            let run = service
                .record_run(&automation, unix_time(), AutomationTrigger::Manual)
                .unwrap();
            assert_eq!(
                persisted(&service).runs[0].status,
                AutomationRunStatus::Pending
            );
            let session = Uuid::new_v4();
            begin_run(&service, run.id, automation.id, session);
            let reopened = AutomationService::open(service.store.path.clone()).unwrap();
            assert_eq!(
                reopened.document().runs[0].status,
                AutomationRunStatus::Running
            );
            assert_eq!(reopened.document().runs[0].session_id, Some(session));
            service.finish_run(run.id, status, None, None, "test:terminal");
            assert_eq!(
                AutomationService::open(service.store.path.clone())
                    .unwrap()
                    .document()
                    .runs[0]
                    .status,
                status
            );
            assert_eq!(persisted(&service).runs[0].id, run.id);
        }
        service
            .record_refusal(
                &automation,
                11,
                AutomationTrigger::Scheduled,
                AutomationRunStatus::SkippedMissed,
                "missed",
                None,
            )
            .unwrap();
        let first = persisted(&service).runs[0].clone();
        service
            .record_refusal(
                &automation,
                12,
                AutomationTrigger::Scheduled,
                AutomationRunStatus::SkippedMissed,
                "missed",
                None,
            )
            .unwrap();
        assert_ne!(persisted(&service).runs[0].id, first.id);
        assert!(persisted(&service).runs.contains(&first));
        let run = service
            .record_run(&automation, 13, AutomationTrigger::Manual)
            .unwrap();
        begin_run(&service, run.id, automation.id, Uuid::new_v4());
        let started = persisted(&service).runs[0].started_at.unwrap();
        service.fail_stale_runs(started + STALE_RUN_TIMEOUT_SECONDS + 1);
        assert_eq!(
            persisted(&service).runs[0].status,
            AutomationRunStatus::Failed
        );
        assert_eq!(
            persisted(&service).automations[0].last_run_status,
            Some(AutomationRunStatus::Failed)
        );
        let run = service
            .record_run(&automation, 14, AutomationTrigger::Manual)
            .unwrap();
        let session = Uuid::new_v4();
        begin_run(&service, run.id, automation.id, session);
        service.note_driver_event(
            session,
            persisted(&service).runs[0].turn_id,
            &DriverEvent::ProcessExited,
        );
        assert_eq!(
            persisted(&service).runs[0].status,
            AutomationRunStatus::Failed
        );
    }

    #[test]
    fn failed_running_and_terminal_writes_retry_bookkeeping_never_dispatch() {
        let service = service();
        let automation = service.upsert(input("uncertain")).unwrap();
        let dispatches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let capture = dispatches.clone();
        *service.dispatch_hook.lock() = Some(Arc::new(move |_| {
            capture.fetch_add(1, Ordering::Relaxed);
        }));
        let run = service
            .record_run(&automation, 1, AutomationTrigger::Scheduled)
            .unwrap();
        service.dispatch(&automation, run.id);
        let session = Uuid::new_v4();
        let message = Uuid::new_v4();
        service.bind_message(run.id, session, message).unwrap();
        inject_write_failure(&service);
        assert!(
            service
                .note_prompt_delivered(session, message, Uuid::new_v4())
                .is_err()
        );
        assert_eq!(
            persisted(&service).runs[0].status,
            AutomationRunStatus::Pending
        );
        assert_eq!(
            service.document().runs[0].status,
            AutomationRunStatus::Pending
        );
        // The accepted dispatch worker resolves the failed pre-send save.
        service.finish_run(
            run.id,
            AutomationRunStatus::Failed,
            Some("delivery association could not be saved".into()),
            None,
            "delivery:persistence",
        );
        assert!(
            service.document().runs[0]
                .error
                .as_deref()
                .unwrap()
                .contains("uncertain")
        );
        assert_eq!(
            service.lifecycle.lock().repair.as_ref().unwrap().runs[0].session_id,
            Some(session)
        );
        service.finish_run(run.id, AutomationRunStatus::Completed, None, None, "");
        assert_eq!(
            service.lifecycle.lock().repair.as_ref().unwrap().runs[0].status,
            AutomationRunStatus::Failed
        );
        assert!(service.run_now(automation.id).is_err());
        allow_writes(&service);
        service.reconcile_interrupted_runs().unwrap();
        let stored = persisted(&service);
        assert_eq!(stored.runs[0].id, run.id);
        assert_eq!(stored.runs[0].session_id, Some(session));
        assert_eq!(stored.runs[0].status, AutomationRunStatus::Failed);
        service.dispatch(&automation, run.id);
        assert_eq!(dispatches.load(Ordering::Relaxed), 1);
        assert!(service.lifecycle.lock().repair.is_none());

        // Separately fail a success write after Running was committed.
        let run = service
            .record_run(&automation, 2, AutomationTrigger::Scheduled)
            .unwrap();
        service.dispatch(&automation, run.id);
        begin_run(&service, run.id, automation.id, session);
        inject_write_failure(&service);
        service.finish_run(run.id, AutomationRunStatus::Completed, None, None, "");
        assert_eq!(
            service.document().runs[0].status,
            AutomationRunStatus::Running
        );
        assert_eq!(
            persisted(&service).runs[0].status,
            AutomationRunStatus::Running
        );
        service.finish_run(
            run.id,
            AutomationRunStatus::Failed,
            Some("late exit".into()),
            None,
            "exit",
        );
        allow_writes(&service);
        service.reconcile_interrupted_runs().unwrap();
        assert_eq!(
            persisted(&service).runs[0].status,
            AutomationRunStatus::Completed
        );
        service.dispatch(&automation, run.id);
        assert_eq!(dispatches.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn failed_worker_creation_resolves_the_durable_run_even_if_its_save_fails() {
        let service = service();
        let automation = service.upsert(input("worker")).unwrap();
        service.fail_worker.store(true, Ordering::Relaxed);
        assert!(service.run_now(automation.id).is_err());
        assert_eq!(
            persisted(&service).runs[0].status,
            AutomationRunStatus::Failed
        );
        let run = service
            .record_run(&automation, 2, AutomationTrigger::Manual)
            .unwrap();
        inject_write_failure(&service);
        assert!(service.spawn_dispatch(automation, run.id).is_err());
        assert_eq!(
            persisted(&service).runs[0].status,
            AutomationRunStatus::Pending
        );
        assert!(
            service.document().runs[0]
                .error
                .as_deref()
                .unwrap()
                .contains("uncertain")
        );
        allow_writes(&service);
        service.reconcile_interrupted_runs().unwrap();
        assert_eq!(
            persisted(&service).runs[0].status,
            AutomationRunStatus::Failed
        );
        assert_eq!(persisted(&service).runs[0].id, run.id);
    }

    #[test]
    fn restart_windows_fail_conservatively_and_reconciliation_failure_blocks_dispatch() {
        for boundary in [
            "pending",
            "dispatched",
            "running",
            "unsaved-terminal",
            "terminal",
        ] {
            let service = service();
            let mut create = input("restart");
            create.webhook = true;
            let automation = service.upsert(create).unwrap();
            let dispatches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let capture = dispatches.clone();
            *service.dispatch_hook.lock() = Some(Arc::new(move |_| {
                capture.fetch_add(1, Ordering::Relaxed);
            }));
            let run = service
                .record_run(&automation, 1, AutomationTrigger::Scheduled)
                .unwrap();
            if boundary != "pending" {
                service.dispatch(&automation, run.id);
            }
            if ["running", "unsaved-terminal", "terminal"].contains(&boundary) {
                begin_run(&service, run.id, automation.id, Uuid::new_v4());
            }
            if boundary == "unsaved-terminal" {
                inject_write_failure(&service);
            }
            if ["unsaved-terminal", "terminal"].contains(&boundary) {
                service.finish_run(run.id, AutomationRunStatus::Completed, None, None, "");
            }
            let restored = Arc::new(AutomationService::open(service.store.path.clone()).unwrap());
            if boundary != "terminal" {
                inject_write_failure(&restored);
                assert!(restored.reconcile_interrupted_runs().is_err());
                assert!(!persisted(&restored).runs[0].status.is_terminal());
                assert!(!restored.document().runs[0].status.is_terminal());
                assert!(restored.run_now(automation.id).is_err());
                assert!(
                    restored
                        .trigger_webhook(
                            automation.id,
                            automation.webhook_secret.as_deref().unwrap()
                        )
                        .is_err()
                );
                assert!(restored.claim_dispatch(run.id).is_err());
                restored.evaluate(utc(2026, 10, 11, 9, 0, 0));
                assert_eq!(persisted(&restored).runs.len(), 1);
                allow_writes(&restored);
            }
            restored.reconcile_interrupted_runs().unwrap();
            let stored = persisted(&restored);
            assert_eq!(stored.runs[0].id, run.id);
            assert_eq!(stored.runs[0].scheduled_for, 1);
            assert_eq!(
                stored.runs[0].status,
                if boundary == "terminal" {
                    AutomationRunStatus::Completed
                } else {
                    AutomationRunStatus::Failed
                }
            );
            assert_eq!(
                stored.automations[0].last_run_status,
                Some(stored.runs[0].status)
            );
            assert!(!restored.claim_dispatch(run.id).unwrap());
            assert_eq!(
                dispatches.load(Ordering::Relaxed),
                usize::from(boundary != "pending")
            );
        }
    }

    #[test]
    fn save_and_publication_are_serialized_across_competing_candidates() {
        use std::sync::{Barrier, mpsc};
        let service = service();
        let automation = service.upsert(input("original")).unwrap();
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let saves = Arc::new(Mutex::new(Vec::new()));
        let capture = saves.clone();
        let enter = entered.clone();
        let leave = release.clone();
        *service.store.before_save.lock() = Some(Arc::new(move |state| {
            if state.automations[0].name == "first" {
                enter.wait();
                leave.wait();
            }
            capture.lock().push(state.automations[0].name.clone());
            Ok(())
        }));
        let publications = Arc::new(Mutex::new(Vec::new()));
        let capture = publications.clone();
        let path = service.store.path.clone();
        service.set_sink(Arc::new(move |state| {
            let disk: AutomationsState = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(disk, state);
            capture.lock().push(state.automations[0].name.clone());
        }));
        let first = service.clone();
        let worker = std::thread::spawn(move || {
            let mut edit = input("first");
            edit.id = Some(automation.id);
            first.upsert(edit).unwrap();
        });
        entered.wait();
        let second = service.clone();
        let (sent, received) = mpsc::channel();
        let follower = std::thread::spawn(move || {
            sent.send(()).unwrap();
            let mut edit = input("second");
            edit.id = Some(automation.id);
            second.upsert(edit).unwrap();
        });
        received.recv().unwrap();
        release.wait();
        worker.join().unwrap();
        follower.join().unwrap();
        assert_eq!(*saves.lock(), ["first", "second"]);
        assert_eq!(*publications.lock(), ["first", "second"]);
        assert_eq!(persisted(&service).automations[0].name, "second");
    }

    #[test]
    fn legacy_json_preserves_history_and_optional_fields_through_reconciliation() {
        let service = service();
        let automation = service.upsert(input("legacy")).unwrap();
        let historical = service
            .record_run(&automation, 10, AutomationTrigger::Scheduled)
            .unwrap();
        service.finish_run(
            historical.id,
            AutomationRunStatus::SkippedPrecheck,
            None,
            None,
            "legacy",
        );
        let pending = service
            .record_run(&automation, 11, AutomationTrigger::Scheduled)
            .unwrap();
        let mut json = serde_json::to_value(persisted(&service)).unwrap();
        let rows = json["runs"].as_array_mut().unwrap();
        for row in rows.iter_mut() {
            for field in [
                "sessionId",
                "messageId",
                "turnId",
                "startedAt",
                "precheck",
                "error",
            ] {
                row.as_object_mut().unwrap().remove(field);
            }
        }
        rows[1]["refusalCount"] = serde_json::json!(7);
        let historical_json = rows[1].clone();
        fs::write(&service.store.path, serde_json::to_vec(&json).unwrap()).unwrap();
        let restored = AutomationService::open(service.store.path.clone()).unwrap();
        assert_eq!(restored.document().runs[0].session_id, None);
        assert_eq!(restored.document().runs[1].refusal_count, 7);
        let original_terminal: AutomationRun = serde_json::from_value(historical_json).unwrap();
        restored.reconcile_interrupted_runs().unwrap();
        let state = persisted(&restored);
        assert_eq!(state.runs[0].id, pending.id);
        assert_eq!(state.runs[0].status, AutomationRunStatus::Failed);
        assert_eq!(state.runs[0].session_id, None);
        assert_eq!(state.runs[1], original_terminal);
        // The same bytes remain readable by the unchanged public JSON type.
        let _: AutomationsState =
            serde_json::from_slice(&fs::read(&restored.store.path).unwrap()).unwrap();
    }

    #[test]
    fn history_retirement_keeps_scheduled_watermark_and_live_run_identity() {
        let service = service();
        let automation = service.upsert(input("retention")).unwrap();
        service
            .record_refusal(
                &automation,
                100,
                AutomationTrigger::Scheduled,
                AutomationRunStatus::SkippedMissed,
                "missed",
                None,
            )
            .unwrap();
        let watermark = persisted(&service).runs[0].id;
        let live = service
            .record_run(&automation, 101, AutomationTrigger::Manual)
            .unwrap();
        for occurrence in 102..(102 + MAX_RUNS_PER_AUTOMATION as u64 + 2) {
            service
                .record_refusal(
                    &automation,
                    occurrence,
                    AutomationTrigger::Manual,
                    AutomationRunStatus::SkippedUnavailable,
                    "unavailable",
                    None,
                )
                .unwrap();
        }
        let state = persisted(&service);
        assert!(state.runs.iter().any(|run| run.id == watermark));
        assert!(state.runs.iter().any(|run| run.id == live.id));
        assert_eq!(
            state
                .runs
                .iter()
                .filter(|run| run.status.is_terminal())
                .count(),
            MAX_RUNS_PER_AUTOMATION
        );
        let restored = AutomationService::open(service.store.path.clone()).unwrap();
        restored.reconcile_interrupted_runs().unwrap();
        assert!(
            persisted(&restored)
                .runs
                .iter()
                .any(|run| run.id == live.id && run.status == AutomationRunStatus::Failed)
        );
        assert!(
            restored
                .record_run(&automation, 99, AutomationTrigger::Scheduled)
                .is_err()
        );
        assert!(
            restored
                .record_run(&automation, 100, AutomationTrigger::Scheduled)
                .is_err()
        );
        assert!(
            persisted(&restored)
                .runs
                .iter()
                .any(|run| run.id == watermark)
        );
    }
}
