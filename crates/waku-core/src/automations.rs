//! Daemon-owned scheduled automations: definitions, run history, and the
//! tick that fires them.
//!
//! The daemon owns the whole lifecycle so schedules fire whether or not a
//! desktop is attached. [`AutomationService`] keeps the document behind one
//! mutex — definitions plus a bounded run history — persists it to
//! `automations.json` beside the task database, and broadcasts the whole
//! document on every change. A tick thread evaluates due schedules on a
//! fixed cadence; each evaluation is idempotent, so a restart, a sleep
//! catch-up, and a manual "run now" can never double-fire one occurrence.

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

use crate::daemon::WakuBackend;
use crate::model::DriverEvent;
use crate::server::EventSink;

/// Broadcast channel the server installs: the whole automations document on
/// every change.
pub type AutomationsSink = Arc<dyn Fn(AutomationsState) + Send + Sync>;

/// One scheduler pass every this often. Sub-minute schedules would need a
/// faster cadence; the tick itself is cheap, the bound is scheduling jitter.
const TICK_INTERVAL: Duration = Duration::from_secs(45);
/// Runs kept per automation — enough history to audit a broken schedule
/// without letting an hourly job's document grow without bound.
const MAX_RUNS_PER_AUTOMATION: usize = 50;
/// A running run whose turn never reports gets this long before the tick
/// fails it; providers can legitimately work for hours, so the bound stays
/// generous.
const STALE_RUN_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;

/// JSON persistence for the automations document — same quarantine +
/// atomic-write pattern as [`crate::settings::DaemonSettingsStore`].
pub struct AutomationsStore {
    path: PathBuf,
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
        Ok((Self { path }, state))
    }

    fn save(&self, state: &AutomationsState) -> io::Result<()> {
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

/// Definition edits, run bookkeeping, and the scheduler tick.
pub struct AutomationService {
    document: Mutex<AutomationsState>,
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
    stop: Arc<(StdMutex<bool>, Condvar)>,
}

impl AutomationService {
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let (store, state) = AutomationsStore::open(path)?;
        Ok(Self {
            document: Mutex::new(state),
            store,
            sink: Mutex::new(None),
            backend: Mutex::new(Weak::new()),
            event_source: Mutex::new(None),
            task_notifier: Mutex::new(None),
            evaluating: AtomicBool::new(false),
            started: AtomicBool::new(false),
            stop: Arc::new((StdMutex::new(false), Condvar::new())),
        })
    }

    pub fn document(&self) -> AutomationsState {
        self.document.lock().clone()
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

    /// Push the current document to every subscriber.
    fn publish(&self) {
        let sink = self.sink.lock().clone();
        if let Some(sink) = sink {
            sink(self.document.lock().clone());
        }
    }

    /// Persist then publish — every mutation funnels here.
    fn commit(&self) -> anyhow::Result<()> {
        self.store.save(&self.document.lock())?;
        self.publish();
        Ok(())
    }

    /// Create or replace one automation. Validates the schedule, timezone,
    /// and target up front so a broken definition can never sit enabled and
    /// silently do nothing.
    pub fn upsert(&self, input: AutomationInput) -> anyhow::Result<Automation> {
        validate_input(&input)?;
        let now = unix_time();
        let mut document = self.document.lock();
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
        drop(document);
        self.commit()?;
        Ok(saved)
    }

    /// Delete a definition and every run it ever recorded.
    pub fn remove(&self, automation_id: Uuid) -> anyhow::Result<()> {
        {
            let mut document = self.document.lock();
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
        }
        self.commit()
    }

    /// Queue a manual run. Returns the recorded run immediately; dispatch
    /// proceeds on a worker thread so the RPC never blocks on a precheck.
    pub fn run_now(self: &Arc<Self>, automation_id: Uuid) -> anyhow::Result<AutomationRun> {
        let automation = {
            let document = self.document.lock();
            document
                .automations
                .iter()
                .find(|automation| automation.id == automation_id)
                .cloned()
                .ok_or_else(|| anyhow!("automation {automation_id} is unknown to the daemon"))?
        };
        let run = self.record_run(&automation, unix_time(), AutomationTrigger::Manual);
        self.spawn_dispatch(automation, run.id);
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
            let document = self.document.lock();
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
            );
            bail!("automation {automation_id} is disabled");
        }
        let run = self.record_run(&automation, unix_time(), AutomationTrigger::Webhook);
        self.spawn_dispatch(automation, run.id);
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
        self.reconcile_interrupted_runs();
        let service = Arc::downgrade(self);
        let stop = self.stop.clone();
        std::thread::Builder::new()
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
            .ok();
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
        let now_s = now.timestamp().max(0) as u64;
        let due: Vec<Automation> = {
            let document = self.document.lock();
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
        let Some(occurrence) = latest_occurrence(&compiled, &zone, Utc::now()) else {
            return Ok(());
        };
        if occurrence < automation.created_at || occurrence > now_s {
            return Ok(());
        }
        {
            let document = self.document.lock();
            if document.runs.iter().any(|run| {
                run.automation_id == automation.id
                    && run.trigger == AutomationTrigger::Scheduled
                    && run.scheduled_for == occurrence
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
                let run = self.record_run(automation, occurrence, AutomationTrigger::Scheduled);
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
                );
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
    ) -> AutomationRun {
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
            error: None,
            precheck: None,
            refusal_key: None,
            refusal_count: 0,
            created_at: now,
            updated_at: now,
        };
        {
            let mut document = self.document.lock();
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
            Self::trim_runs(&mut document, automation.id);
        }
        self.publish();
        run
    }

    /// A run that reached no task: identical consecutive refusals fold into
    /// the previous row's `refusal_count` so a schedule stuck unavailable
    /// cannot flood history.
    fn record_refusal(
        &self,
        automation: &Automation,
        occurrence: u64,
        trigger: AutomationTrigger,
        status: AutomationRunStatus,
        refusal_key: &str,
        error: Option<String>,
    ) {
        let now = unix_time();
        let mut document = self.document.lock();
        let last = document
            .runs
            .iter()
            .position(|run| run.automation_id == automation.id);
        if let Some(index) = last {
            let run = &mut document.runs[index];
            if run.refusal_key.as_deref() == Some(refusal_key) && run.status == status {
                run.refusal_count = run.refusal_count.saturating_add(1);
                run.updated_at = now;
                if let Some(entry) = document
                    .automations
                    .iter_mut()
                    .find(|entry| entry.id == automation.id)
                {
                    entry.last_run_at = Some(now);
                    entry.last_run_status = Some(status);
                    entry.updated_at = now;
                }
                drop(document);
                self.publish();
                return;
            }
        }
        let run = AutomationRun {
            id: Uuid::new_v4(),
            automation_id: automation.id,
            trigger,
            status,
            scheduled_for: occurrence,
            started_at: Some(now),
            finished_at: Some(now),
            session_id: None,
            error,
            precheck: None,
            refusal_key: Some(refusal_key.to_owned()),
            refusal_count: 0,
            created_at: now,
            updated_at: now,
        };
        document.runs.insert(0, run);
        if let Some(entry) = document
            .automations
            .iter_mut()
            .find(|entry| entry.id == automation.id)
        {
            entry.last_run_at = Some(now);
            entry.last_run_status = Some(status);
            entry.last_refusal_key = Some(refusal_key.to_owned());
            entry.updated_at = now;
        }
        Self::trim_runs(&mut document, automation.id);
        drop(document);
        self.publish();
    }

    /// Dispatch on a dedicated thread — a precheck or worktree creation can
    /// block, and neither the RPC handler nor the tick loop should stall on
    /// it.
    fn spawn_dispatch(self: &Arc<Self>, automation: Automation, run_id: Uuid) {
        let service = self.clone();
        std::thread::Builder::new()
            .name(format!("goddard-automation-{}", automation.id))
            .spawn(move || service.dispatch(&automation, run_id))
            .ok();
    }

    /// Run precheck + dispatch for a recorded run. Called synchronously from
    /// `evaluate_one` (the `evaluating` flag already serializes evaluations)
    /// and from `run_now`'s worker.
    fn dispatch(&self, automation: &Automation, run_id: Uuid) {
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
                let result = backend.create_agent_task(
                    None,
                    automation.provider,
                    automation.model.clone().unwrap_or_default(),
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
                    Ok(session_id) => self.begin_run(run_id, automation.id, session_id),
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
        match backend.queue_agent_prompt(target, automation.prompt.clone(), None, events) {
            Ok(()) => {
                self.notify_tasks_changed();
                self.begin_run(run_id, automation.id, target)
            }
            Err(error) => self.finish_run(
                run_id,
                AutomationRunStatus::Failed,
                Some(format!("{error:#}")),
                None,
                "dispatch:failed",
            ),
        }
    }

    /// The prompt reached a task: the run waits for the session's next
    /// finished turn.
    fn begin_run(&self, run_id: Uuid, automation_id: Uuid, session_id: Uuid) {
        let now = unix_time();
        let mut document = self.document.lock();
        if let Some(run) = document.runs.iter_mut().find(|run| run.id == run_id) {
            run.status = AutomationRunStatus::Running;
            run.session_id = Some(session_id);
            run.started_at = Some(now);
            run.updated_at = now;
        }
        if let Some(automation) = document
            .automations
            .iter_mut()
            .find(|entry| entry.id == automation_id)
        {
            automation.last_session_id = Some(session_id);
            automation.last_run_status = Some(AutomationRunStatus::Running);
            automation.last_refusal_key = None;
            automation.updated_at = now;
        }
        drop(document);
        self.publish();
    }

    /// Terminal write for a run that never reached — or left — a task.
    fn finish_run(
        &self,
        run_id: Uuid,
        status: AutomationRunStatus,
        error: Option<String>,
        precheck: Option<AutomationPrecheckResult>,
        refusal_key: &str,
    ) {
        let now = unix_time();
        let mut document = self.document.lock();
        let Some(automation_id) = document
            .runs
            .iter()
            .find(|run| run.id == run_id)
            .map(|run| run.automation_id)
        else {
            return;
        };
        // Refusal coalescing applies to Pending runs too: if the previous
        // run folded for the same reason, drop this run row into it.
        let coalesce = document
            .runs
            .iter()
            .find(|run| {
                run.automation_id == automation_id
                    && run.id != run_id
                    && run.refusal_key.as_deref() == Some(refusal_key)
                    && run.status == status
            })
            .map(|run| run.id);
        if let Some(fold_into) = coalesce {
            document.runs.retain(|run| run.id != run_id);
            if let Some(previous) = document.runs.iter_mut().find(|run| run.id == fold_into) {
                previous.refusal_count = previous.refusal_count.saturating_add(1);
                previous.updated_at = now;
            }
        } else if let Some(run) = document.runs.iter_mut().find(|run| run.id == run_id) {
            run.status = status;
            run.error = error;
            run.precheck = precheck;
            run.finished_at = Some(now);
            run.refusal_key = Some(refusal_key.to_owned());
            run.updated_at = now;
        }
        if let Some(automation) = document
            .automations
            .iter_mut()
            .find(|entry| entry.id == automation_id)
        {
            automation.last_run_status = Some(status);
            automation.last_refusal_key = Some(refusal_key.to_owned());
            automation.updated_at = now;
        }
        drop(document);
        self.publish();
    }

    /// Runtime event hook — a finished turn settles the session's running
    /// run; a dead runtime fails it outright.
    pub fn note_driver_event(&self, session_id: Uuid, event: &DriverEvent) {
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
        let now = unix_time();
        let mut document = self.document.lock();
        let mut changed = false;
        let mut automation_id = None;
        if let Some(run) = document.runs.iter_mut().find(|run| {
            run.session_id == Some(session_id) && run.status == AutomationRunStatus::Running
        }) {
            run.status = status;
            run.error = error;
            run.finished_at = Some(now);
            run.updated_at = now;
            automation_id = Some(run.automation_id);
            changed = true;
        }
        if let Some(automation_id) = automation_id {
            if let Some(automation) = document
                .automations
                .iter_mut()
                .find(|entry| entry.id == automation_id)
            {
                automation.last_run_status = Some(status);
                automation.updated_at = now;
            }
        }
        drop(document);
        if changed {
            self.publish();
        }
    }

    /// A run left Pending or Running across a daemon restart can never
    /// finish — its dispatch thread and runtime are gone — so it fails
    /// honestly at boot rather than hanging forever.
    fn reconcile_interrupted_runs(&self) {
        let now = unix_time();
        let mut document = self.document.lock();
        let mut changed = false;
        for run in document.runs.iter_mut() {
            if !run.status.is_terminal() {
                run.status = AutomationRunStatus::Failed;
                run.error = Some("the daemon restarted before the run finished".to_owned());
                run.finished_at = Some(now);
                run.updated_at = now;
                changed = true;
            }
        }
        drop(document);
        if changed {
            let _ = self.commit();
        }
    }

    /// A running run whose session produced no turn for a day is orphaned
    /// bookkeeping — the daemon lost the thread that would settle it.
    fn fail_stale_runs(&self, now_s: u64) {
        let mut document = self.document.lock();
        let mut changed = false;
        for run in document.runs.iter_mut() {
            if run.status != AutomationRunStatus::Running {
                continue;
            }
            let stale = run
                .started_at
                .is_some_and(|started| now_s.saturating_sub(started) > STALE_RUN_TIMEOUT_SECONDS);
            if stale {
                run.status = AutomationRunStatus::Failed;
                run.error = Some("the run timed out waiting for the task".to_owned());
                run.finished_at = Some(now_s);
                run.updated_at = now_s;
                changed = true;
            }
        }
        drop(document);
        if changed {
            self.publish();
        }
    }

    /// Recompute `next_run_at` for every scheduled automation — displayed in
    /// the UI and read by the next evaluation's dedupe check.
    fn refresh_next_run_at(&self, now: DateTime<Utc>) {
        let mut document = self.document.lock();
        let mut changed = false;
        for automation in document.automations.iter_mut() {
            let next = automation.schedule.as_ref().and_then(|schedule| {
                compile_schedule(schedule).ok().and_then(|compiled| {
                    resolve_zone(&automation.timezone)
                        .ok()
                        .and_then(|zone| next_occurrence(&compiled, &zone, now))
                })
            });
            let next = automation.enabled.then_some(next).flatten();
            if automation.next_run_at != next {
                automation.next_run_at = next;
                changed = true;
            }
        }
        drop(document);
        if changed {
            let _ = self.commit();
        }
    }

    /// Keep only the newest `MAX_RUNS_PER_AUTOMATION` rows for one
    /// automation. `runs` stays newest-first, so trimming drops the tail.
    fn trim_runs(document: &mut AutomationsState, automation_id: Uuid) {
        let mut kept = 0usize;
        document.runs.retain(|run| {
            if run.automation_id != automation_id {
                return true;
            }
            kept += 1;
            kept <= MAX_RUNS_PER_AUTOMATION
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
    let mut child = match crate::command_env::spawn(&mut command) {
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
            project_path: PathBuf::from("/tmp"),
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
}
