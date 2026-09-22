//! Provider-cloud driver: a `DriverControl` whose transport is a remote
//! task rather than a local process.
//!
//! Ordering stays single-threaded — every `DriverControl` call lands on a
//! command channel the worker drains, and backend watchers report through
//! the same channel, so event order never races. The worker emits ordinary
//! `DriverEvent`s (`Connected`, `TurnStarted`, `TextDelta`, `Activity`,
//! `TurnParked`, `TurnFinished`) — a cloud session looks like any other
//! session to clients, replay journals, and persistence.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use anyhow::anyhow;
use crossbeam_channel::{Sender, unbounded};

use super::{
    AgentSurfaceDelivery, DriverControl, DriverEventSender, DriverStartOptions, SessionOptions,
};
use crate::cloud::{
    BackendEvent, CloudBackend, CloudLaunch, CloudTarget, LaunchContext, SendOutcome, backend_for,
    resolve_target,
};
use crate::model::{ActivityKind, DriverEvent, ProviderKind, ProviderResumeCursor};

/// The one transcript row that tracks the remote task — every update
/// carries the same id, so the transcript shows one live status instead of
/// a stream of near-identical rows.
const STATUS_ACTIVITY_ID: &str = "cloud-status";

enum Command {
    Prompt(String),
    Steer(String),
    Cancel,
    /// A watcher's observation, funneled onto the worker's ordering.
    Backend(BackendEvent),
    Shutdown,
}

pub struct CloudDriver {
    commands: Sender<Command>,
    /// Devin and Cursor accept messages mid-flight; the rest reject steers.
    steerable: bool,
}

/// Everything the worker knows about the remote task.
struct WorkerState {
    provider: ProviderKind,
    launch: Option<CloudLaunch>,
    target: Option<CloudTarget>,
    /// A Goddard turn is open on the remote task — set by TurnStarted,
    /// cleared by TurnFinished.
    turn_open: bool,
    /// The open turn is parked — the remote side is waiting for input, and
    /// a message continues it rather than opening a new turn.
    parked: bool,
    /// The remote task settled for good — the next prompt submits a fresh
    /// task rather than messaging this one.
    settled: bool,
    watch_stop: Option<Arc<AtomicBool>>,
}

impl WorkerState {
    /// The remote task is mid-turn — a prompt cannot join it.
    fn running(&self) -> bool {
        self.launch.is_some() && self.turn_open && !self.parked && !self.settled
    }

    /// An idle-but-alive remote task that a message can still reach —
    /// including, for Copilot, a task whose run finished but whose PR
    /// keeps taking `@copilot` comments.
    fn messaging_possible(&self, backend: &dyn CloudBackend) -> bool {
        self.launch.is_some() && (!self.settled || backend.finished_accepts_messages())
    }
}

/// One `backend.watch` per launch on its own thread, reporting through the
/// shared sink; the returned flag ends it early.
fn spawn_watch(
    backend: Arc<dyn CloudBackend>,
    launch: CloudLaunch,
    sink: Sender<BackendEvent>,
) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    thread::spawn(move || backend.watch(&launch, sink, flag));
    stop
}

fn connected_event(launch: &CloudLaunch) -> DriverEvent {
    DriverEvent::Connected {
        provider_cursor: Some(launch.cursor.clone()),
    }
}

impl CloudDriver {
    pub fn start(
        provider: ProviderKind,
        options: DriverStartOptions,
        events: DriverEventSender,
    ) -> anyhow::Result<Self> {
        let backend: Arc<dyn CloudBackend> = backend_for(provider).ok_or_else(|| {
            anyhow!(
                "{} has no hosted cloud environment — pick This Mac, the Sandbox VM, or another provider",
                provider.display_name()
            )
        })?;
        let ctx = LaunchContext {
            binary: options.binary,
            cwd: options.cwd,
        };
        let steerable = backend.supports_messages();
        let (commands, receiver) = unbounded::<Command>();
        let (backend_sink, backend_events) = unbounded::<BackendEvent>();
        // Watchers and the worker share one ordering by forwarding every
        // backend observation into the command channel.
        {
            let commands = commands.clone();
            thread::spawn(move || {
                while let Ok(event) = backend_events.recv() {
                    if commands.send(Command::Backend(event)).is_err() {
                        return;
                    }
                }
            });
        }
        // A resumed session reattaches to its remote task instead of
        // submitting a new one. The repo context is still resolved — some
        // backends need its slug to rebuild their watch.
        let mut launch = options
            .provider_cursor
            .as_ref()
            .and_then(|cursor| backend.launch_from_cursor(cursor));
        let mut target = None;
        if launch.is_some()
            && let Ok(resolved) = resolve_target(&ctx.cwd)
        {
            if let Some(launch) = &mut launch
                && let Some(slug) = &resolved.github_slug
                && launch.extra.get("slug").is_none()
            {
                launch.extra["slug"] = serde_json::json!(slug);
            }
            target = Some(resolved);
        }
        let watch_stop = launch
            .as_ref()
            .map(|launch| spawn_watch(backend.clone(), launch.clone(), backend_sink.clone()));
        {
            let events = events.clone();
            let backend = backend.clone();
            thread::spawn(move || {
                let mut state = WorkerState {
                    provider,
                    launch,
                    target,
                    turn_open: false,
                    parked: false,
                    settled: false,
                    watch_stop,
                };
                // Reattach: the remote task still exists, so mark the
                // runtime resumable right away. A fresh launch emits the
                // same event once submit lands.
                if let Some(launch) = state.launch.clone() {
                    Self::emit(&events, &mut state, connected_event(&launch));
                }
                while let Ok(command) = receiver.recv() {
                    match command {
                        Command::Prompt(text) => {
                            Self::prompt(&backend, &ctx, &events, &mut state, &text, &backend_sink);
                        }
                        Command::Steer(text) => {
                            Self::steer(&backend, &events, &mut state, &text, &backend_sink);
                        }
                        Command::Cancel => Self::cancel(&backend, &events, &mut state),
                        Command::Backend(event) => {
                            Self::backend_event(&events, &mut state, event);
                        }
                        Command::Shutdown => {
                            if let Some(stop) = state.watch_stop.take() {
                                stop.store(true, Ordering::Relaxed);
                            }
                            return;
                        }
                    }
                }
            });
        }
        Ok(Self {
            commands,
            steerable,
        })
    }

    /// Emit an event and mirror the turn transitions it implies so the
    /// worker's state stays honest.
    fn emit(events: &DriverEventSender, state: &mut WorkerState, event: DriverEvent) {
        match &event {
            DriverEvent::TurnStarted => {
                state.turn_open = true;
                state.parked = false;
            }
            DriverEvent::TurnParked => state.parked = true,
            DriverEvent::TurnFinished { .. } => {
                state.turn_open = false;
                state.parked = false;
            }
            _ => {}
        }
        let _ = events.send(event);
    }

    /// Update the remote task's status row in place.
    fn status_activity(
        events: &DriverEventSender,
        state: &WorkerState,
        detail: Option<String>,
        complete: bool,
    ) {
        let _ = events.send(DriverEvent::Activity {
            id: Some(STATUS_ACTIVITY_ID.into()),
            kind: ActivityKind::Tool,
            title: format!("{} Cloud", state.provider.display_name()),
            detail,
            complete,
        });
    }

    /// Replace the watcher — the old one is stopped first so a respawned
    /// launch never reports twice.
    fn start_watch(
        backend: &Arc<dyn CloudBackend>,
        state: &mut WorkerState,
        sink: &Sender<BackendEvent>,
    ) {
        if let Some(stop) = state.watch_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(launch) = &state.launch {
            state.watch_stop = Some(spawn_watch(backend.clone(), launch.clone(), sink.clone()));
        }
    }

    fn prompt(
        backend: &Arc<dyn CloudBackend>,
        ctx: &LaunchContext,
        events: &DriverEventSender,
        state: &mut WorkerState,
        text: &str,
        sink: &Sender<BackendEvent>,
    ) {
        if state.running() {
            if backend.supports_messages() {
                Self::message(backend, events, state, text, sink);
            } else {
                Self::emit(
                    events,
                    state,
                    DriverEvent::steer_rejected_keyed(
                        text.to_owned(),
                        localized!(
                            "cloud.still_running",
                            provider = state.provider.display_name()
                        ),
                    ),
                );
            }
            return;
        }
        if state.messaging_possible(backend.as_ref()) && backend.supports_messages() {
            // The remote task is idle-but-alive — a message continues it
            // rather than spending a new task.
            Self::message(backend, events, state, text, sink);
            return;
        }
        // A fresh task: nothing submitted yet, or the previous one settled.
        Self::emit(events, state, DriverEvent::TurnStarted);
        if state.target.is_none() {
            match resolve_target(&ctx.cwd) {
                Ok(target) => state.target = Some(target),
                Err(error) => {
                    Self::emit(
                        events,
                        state,
                        DriverEvent::TurnFinished {
                            success: false,
                            summary: Some(format!("{error:#}")),
                            summary_i18n: None,
                        },
                    );
                    state.settled = true;
                    return;
                }
            }
        }
        let target = state.target.as_ref().expect("resolved above");
        match backend.launch(ctx, target, text) {
            Ok(launch) => {
                Self::status_activity(
                    events,
                    state,
                    launch.detail.clone().or_else(|| launch.url.clone()),
                    false,
                );
                let event = connected_event(&launch);
                state.launch = Some(launch);
                Self::emit(events, state, event);
                Self::start_watch(backend, state, sink);
            }
            Err(error) => {
                Self::emit(
                    events,
                    state,
                    DriverEvent::TurnFinished {
                        success: false,
                        summary: Some(format!("{error:#}")),
                        summary_i18n: None,
                    },
                );
                state.settled = true;
            }
        }
    }

    fn message(
        backend: &Arc<dyn CloudBackend>,
        events: &DriverEventSender,
        state: &mut WorkerState,
        text: &str,
        sink: &Sender<BackendEvent>,
    ) {
        let Some(launch) = state.launch.clone() else {
            return;
        };
        match backend.send_message(&launch, text) {
            Ok(SendOutcome::Delivered) => {
                // A delivered message means the task is alive again —
                // parked turn resumes, finished task reactivates.
                state.settled = false;
                if !state.turn_open || state.parked {
                    Self::emit(events, state, DriverEvent::TurnStarted);
                }
                // A message can wake a task whose watcher ended with its
                // last run — Copilot's PR channel works exactly this way.
                if state.watch_stop.is_none() {
                    Self::start_watch(backend, state, sink);
                }
            }
            Ok(SendOutcome::Respawned(relaunch)) => {
                state.launch = Some(relaunch);
                Self::emit(events, state, DriverEvent::TurnStarted);
                Self::start_watch(backend, state, sink);
            }
            Err(error) => {
                Self::emit(events, state, DriverEvent::Error(format!("{error:#}")));
            }
        }
    }

    fn steer(
        backend: &Arc<dyn CloudBackend>,
        events: &DriverEventSender,
        state: &mut WorkerState,
        text: &str,
        sink: &Sender<BackendEvent>,
    ) {
        let Some(launch) = state.launch.clone() else {
            Self::emit(
                events,
                state,
                DriverEvent::steer_rejected_keyed(text.to_owned(), localized!("cloud.not_running")),
            );
            return;
        };
        match backend.send_message(&launch, text) {
            Ok(SendOutcome::Delivered) => {
                Self::emit(
                    events,
                    state,
                    DriverEvent::SteerAccepted {
                        message: text.to_owned(),
                        sent_by_task: None,
                        hidden: false,
                    },
                );
            }
            Ok(SendOutcome::Respawned(relaunch)) => {
                state.launch = Some(relaunch);
                Self::emit(
                    events,
                    state,
                    DriverEvent::SteerAccepted {
                        message: text.to_owned(),
                        sent_by_task: None,
                        hidden: false,
                    },
                );
                Self::start_watch(backend, state, sink);
            }
            Err(error) => {
                Self::emit(
                    events,
                    state,
                    DriverEvent::SteerRejected {
                        message: text.to_owned(),
                        reason: format!("{error:#}"),
                        reason_i18n: None,
                        hidden: false,
                    },
                );
            }
        }
    }

    fn cancel(
        backend: &Arc<dyn CloudBackend>,
        events: &DriverEventSender,
        state: &mut WorkerState,
    ) {
        if let Some(launch) = &state.launch {
            let _ = backend.cancel(launch);
        }
        if let Some(stop) = state.watch_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if state.turn_open {
            Self::emit(
                events,
                state,
                DriverEvent::turn_finished_keyed(false, localized!("cloud.stopped")),
            );
        }
        state.settled = true;
    }

    fn backend_event(events: &DriverEventSender, state: &mut WorkerState, event: BackendEvent) {
        match event {
            BackendEvent::Running { detail } => {
                // Running un-parks a waiting turn; a fresh watcher opens one.
                if !state.turn_open || state.parked {
                    Self::emit(events, state, DriverEvent::TurnStarted);
                }
                Self::status_activity(events, state, detail, false);
            }
            BackendEvent::Waiting { detail } => {
                if state.turn_open && !state.parked {
                    Self::emit(events, state, DriverEvent::TurnParked);
                }
                Self::status_activity(events, state, detail, false);
            }
            BackendEvent::Message(text) => {
                Self::emit(events, state, DriverEvent::TextDelta(format!("{text}\n\n")));
            }
            BackendEvent::Progress(detail) => {
                Self::status_activity(events, state, Some(detail), false);
            }
            BackendEvent::Finished { success, summary } => {
                Self::status_activity(events, state, Some(summary.clone()), true);
                Self::emit(
                    events,
                    state,
                    DriverEvent::TurnFinished {
                        success,
                        summary: Some(summary),
                        summary_i18n: None,
                    },
                );
                state.settled = true;
                state.watch_stop = None;
            }
        }
    }
}

impl DriverControl for CloudDriver {
    fn prompt(&self, prompt: String) {
        let _ = self.commands.send(Command::Prompt(prompt));
    }

    fn supports_steer(&self) -> bool {
        self.steerable
    }

    /// Nothing local runs — there is no `goddard-agent` surface to inject.
    fn agent_surface_delivery(&self) -> AgentSurfaceDelivery {
        AgentSurfaceDelivery::Absent
    }

    fn steer(&self, prompt: String) {
        let _ = self.commands.send(Command::Steer(prompt));
    }

    fn cancel(&self) {
        let _ = self.commands.send(Command::Cancel);
    }

    fn respond(&self, _request_id: String, _option_id: String) {}

    /// "/compact" is a provider-harness command — sending it as a message
    /// would make the remote agent parse it as task text.
    fn compact(&self) {}

    fn apply_options(&self, _options: SessionOptions) -> bool {
        // Nothing about the launch is reconfigurable without a new task.
        false
    }

    fn rollback(&self, _turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        anyhow::bail!("cloud tasks can't be rewound")
    }
}

impl Drop for CloudDriver {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}
