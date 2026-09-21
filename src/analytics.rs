//! Privacy-conscious product analytics for release builds.
//!
//! Event creation is a bounded `try_send` on the UI thread. A dedicated
//! worker owns both the Tokio runtime required by `rust-umami`/Reqwest and a
//! single Umami session, so networking, TLS, and response-cache bookkeeping
//! never enter a frame path. Events deliberately contain no prompts, project
//! names or paths, provider output, or provider-account identity. A random
//! installation-scoped ID keeps aggregate sessions coherent across launches.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::Duration;

use rust_umami::{Client, Context};
use serde_json::{Value, json};
use uuid::Uuid;

const EVENT_QUEUE_CAPACITY: usize = 128;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(not(debug_assertions))]
const ENDPOINT: Option<&str> = option_env!("GODDARD_ANALYTICS_ENDPOINT");
#[cfg(debug_assertions)]
const ENDPOINT: Option<&str> = None;

#[cfg(not(debug_assertions))]
const WEBSITE_ID: Option<&str> = option_env!("GODDARD_ANALYTICS_WEBSITE_ID");
#[cfg(debug_assertions)]
const WEBSITE_ID: Option<&str> = None;

/// A cheap handle to the background analytics worker.
#[derive(Clone)]
pub struct Analytics {
    events: SyncSender<Event>,
    enabled: Arc<AtomicBool>,
    available: bool,
}

impl Analytics {
    /// Starts analytics only for release builds with configuration embedded
    /// at compile time. Any build can opt out with
    /// `GODDARD_DISABLE_ANALYTICS=1`.
    pub fn new(language: &'static str, distinct_id: Uuid, sharing_enabled: bool) -> Self {
        let (events, receiver) = sync_channel(EVENT_QUEUE_CAPACITY);
        let available = analytics_available();
        let enabled = Arc::new(AtomicBool::new(available && sharing_enabled));
        if available {
            // If the thread cannot be created, its captured receiver is
            // dropped and future `try_send`s simply observe disconnection.
            // Analytics is never allowed to affect app startup or actions.
            let worker_enabled = Arc::clone(&enabled);
            let _ = std::thread::Builder::new()
                .name("waku-analytics".into())
                .spawn(move || run(receiver, language, distinct_id, worker_enabled));
        } else {
            drop(receiver);
        }
        Self {
            events,
            enabled,
            available,
        }
    }

    /// Enqueues an event without waiting for queue space or network I/O.
    pub fn track(&self, event: Event) {
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        let _ = self.events.try_send(event);
    }

    /// Applies the user's persisted sharing preference. The worker also checks
    /// this flag before sending, so disabling drops events already queued.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled
            .store(self.available && enabled, Ordering::Release);
    }
}

/// The deliberately small product vocabulary sent to Umami.
pub enum Event {
    AppLaunched {
        task_count: usize,
        project_count: usize,
    },
    ProjectAdded,
    RightPanelOpened {
        surface: &'static str,
    },
    GitPanelOpened,
    TaskCreated {
        provider: &'static str,
        workspace: &'static str,
        projectless: bool,
        /// `interactive` | `side_chat` | `imported` | `external` — external
        /// covers sessions another client, the CLI, or an automation made.
        origin: &'static str,
    },
    ProviderSetupFinished {
        provider: &'static str,
        /// `installed` | `not_detected`
        outcome: &'static str,
    },
    UpdateResolved {
        /// `accepted` | `installing` | `up_to_date` | `failed`
        outcome: &'static str,
    },
    RemoteConnectFinished {
        /// `ssh` | `direct`
        transport: &'static str,
        /// `connected` | `failed`
        outcome: &'static str,
    },
    PairingFinished {
        /// `granted` | `no_address` | `declined` | `failed`
        outcome: &'static str,
    },
    DaemonExposureChanged {
        enabled: bool,
    },
    FriendAdded,
    TransferFinished {
        /// `outgoing` | `incoming`
        direction: &'static str,
        /// `done` | `failed` | `cancelled`
        outcome: &'static str,
    },
    AutomationRunFinished {
        /// `scheduled` | `manual` | `webhook`
        trigger: &'static str,
        /// `completed` | `failed` | `skipped_precheck` | `skipped_missed` |
        /// `skipped_unavailable`
        outcome: &'static str,
        duration_seconds: Option<u64>,
    },
    GitActionFinished {
        /// `commit` | `commit_push` | `push` | `sync` | `land` | `rebase` |
        /// `abort_sync`
        action: &'static str,
        /// `completed` | `conflict` | `failed`
        outcome: &'static str,
    },
    TerminalOpened {
        /// `session` | `global` | `command` | `panel`
        kind: &'static str,
    },
    SkillToggled {
        enabled: bool,
        /// `applied` | `failed`
        outcome: &'static str,
    },
    GoalSubmitted {
        replace: bool,
    },
    ComputerUseToggled {
        enabled: bool,
    },
    TurnSubmitted {
        provider: &'static str,
        model: String,
        turn_number: usize,
        workspace: &'static str,
        projectless: bool,
        attachment_count: usize,
        has_input: bool,
    },
    TurnFinished {
        provider: &'static str,
        turn_number: usize,
        outcome: TurnOutcome,
        duration_seconds: u64,
    },
    PermissionResponded {
        provider: &'static str,
        kind: &'static str,
        decision: &'static str,
    },
    ConversationRolledBack {
        provider: &'static str,
        turns: usize,
    },
    ResponseForked {
        provider: &'static str,
        turn_number: usize,
    },
    DaemonRecovery {
        /// `unexpected_exit` | `disconnect` | `rebuild`
        cause: &'static str,
        /// `recovered` | `unreachable`
        outcome: &'static str,
        /// Sessions whose lost runtime the app auto-resumed onto the
        /// replacement connection.
        sessions_resumed: usize,
    },
}

#[derive(Clone, Copy)]
pub enum TurnOutcome {
    Completed,
    Failed,
    Cancelled,
    ProcessExited,
    StartFailed,
    PreparationFailed,
}

impl TurnOutcome {
    fn id(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::ProcessExited => "process_exited",
            Self::StartFailed => "start_failed",
            Self::PreparationFailed => "preparation_failed",
        }
    }
}

impl Event {
    fn into_track(self) -> (&'static str, Value) {
        let (name, mut data) = match self {
            Self::AppLaunched {
                task_count,
                project_count,
            } => (
                "app.launched",
                json!({
                    "taskCount": task_count,
                    "projectCount": project_count,
                }),
            ),
            Self::ProjectAdded => ("project.added", json!({})),
            Self::RightPanelOpened { surface } => (
                "right_panel.opened",
                json!({
                    "surface": surface,
                }),
            ),
            Self::GitPanelOpened => ("git_panel.opened", json!({})),
            Self::TaskCreated {
                provider,
                workspace,
                projectless,
                origin,
            } => (
                "task.created",
                json!({
                    "provider": provider,
                    "workspace": workspace,
                    "projectless": projectless,
                    "origin": origin,
                }),
            ),
            Self::ProviderSetupFinished { provider, outcome } => (
                "provider.setup.finished",
                json!({
                    "provider": provider,
                    "outcome": outcome,
                }),
            ),
            Self::UpdateResolved { outcome } => (
                "app.update.resolved",
                json!({
                    "outcome": outcome,
                }),
            ),
            Self::RemoteConnectFinished { transport, outcome } => (
                "remote.connect.finished",
                json!({
                    "transport": transport,
                    "outcome": outcome,
                }),
            ),
            Self::PairingFinished { outcome } => (
                "daemon.pair.finished",
                json!({
                    "outcome": outcome,
                }),
            ),
            Self::DaemonExposureChanged { enabled } => (
                "daemon.exposure.changed",
                json!({
                    "enabled": enabled,
                }),
            ),
            Self::FriendAdded => ("friend.added", json!({})),
            Self::TransferFinished { direction, outcome } => (
                "friend.transfer.finished",
                json!({
                    "direction": direction,
                    "outcome": outcome,
                }),
            ),
            Self::AutomationRunFinished {
                trigger,
                outcome,
                duration_seconds,
            } => (
                "automation.run.finished",
                json!({
                    "trigger": trigger,
                    "outcome": outcome,
                    "durationSeconds": duration_seconds,
                }),
            ),
            Self::GitActionFinished { action, outcome } => (
                "git.action.finished",
                json!({
                    "action": action,
                    "outcome": outcome,
                }),
            ),
            Self::TerminalOpened { kind } => (
                "terminal.opened",
                json!({
                    "kind": kind,
                }),
            ),
            Self::SkillToggled { enabled, outcome } => (
                "skill.toggled",
                json!({
                    "enabled": enabled,
                    "outcome": outcome,
                }),
            ),
            Self::GoalSubmitted { replace } => (
                "goal.submitted",
                json!({
                    "replace": replace,
                }),
            ),
            Self::ComputerUseToggled { enabled } => (
                "computer_use.toggled",
                json!({
                    "enabled": enabled,
                }),
            ),
            Self::TurnSubmitted {
                provider,
                model,
                turn_number,
                workspace,
                projectless,
                attachment_count,
                has_input,
            } => (
                "provider.turn.sent",
                json!({
                    "provider": provider,
                    "model": model,
                    "turnNumber": turn_number,
                    "workspace": workspace,
                    "projectless": projectless,
                    "attachmentCount": attachment_count,
                    "hasInput": has_input,
                }),
            ),
            Self::TurnFinished {
                provider,
                turn_number,
                outcome,
                duration_seconds,
            } => (
                "provider.turn.finished",
                json!({
                    "provider": provider,
                    "turnNumber": turn_number,
                    "outcome": outcome.id(),
                    "durationSeconds": duration_seconds,
                }),
            ),
            Self::PermissionResponded {
                provider,
                kind,
                decision,
            } => (
                "provider.request.responded",
                json!({
                    "provider": provider,
                    "kind": kind,
                    "decision": decision,
                }),
            ),
            Self::ConversationRolledBack { provider, turns } => (
                "provider.conversation.rolled_back",
                json!({
                    "provider": provider,
                    "turns": turns,
                }),
            ),
            Self::ResponseForked {
                provider,
                turn_number,
            } => (
                "provider.response.forked",
                json!({
                    "provider": provider,
                    "turnNumber": turn_number,
                }),
            ),
            Self::DaemonRecovery {
                cause,
                outcome,
                sessions_resumed,
            } => (
                "daemon.recovery",
                json!({
                    "cause": cause,
                    "outcome": outcome,
                    "sessionsResumed": sessions_resumed,
                }),
            ),
        };

        let properties = data
            .as_object_mut()
            .expect("analytics event data is always an object");
        properties.insert("clientType".into(), json!("desktop"));
        properties.insert("version".into(), json!(env!("CARGO_PKG_VERSION")));
        properties.insert("platform".into(), json!(std::env::consts::OS));
        properties.insert("arch".into(), json!(std::env::consts::ARCH));
        properties.insert(
            "build".into(),
            json!(if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }),
        );
        (name, data)
    }
}

fn analytics_available() -> bool {
    !cfg!(debug_assertions)
        && !env_flag("GODDARD_DISABLE_ANALYTICS")
        && ENDPOINT.is_some_and(|value| !value.trim().is_empty())
        && WEBSITE_ID.is_some_and(|value| !value.trim().is_empty())
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn run(
    receiver: Receiver<Event>,
    language: &'static str,
    distinct_id: Uuid,
    enabled: Arc<AtomicBool>,
) {
    let (Some(endpoint), Some(website_id)) = (ENDPOINT, WEBSITE_ID) else {
        return;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };

    // Client construction may initialize runtime-aware networking helpers,
    // so enter the worker's runtime even though the builder itself is sync.
    let client = {
        let _runtime = runtime.enter();
        Client::builder(endpoint, website_id)
            .default_context(
                Context::new()
                    .hostname("goddardai.org")
                    .url("/desktop")
                    .title("Goddard")
                    .language(language)
                    .os(std::env::consts::OS)
                    .device("desktop"),
            )
            .user_agent(format!(
                "Goddard/{} ({}; {})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
            .timeout(REQUEST_TIMEOUT)
            .build()
    };
    let Ok(client) = client else {
        return;
    };
    let session = client.session().distinct_id(distinct_id.to_string());

    while let Ok(event) = receiver.recv() {
        if !enabled.load(Ordering::Acquire) {
            continue;
        }
        let (name, data) = event.into_track();
        // A failed analytics request is intentionally terminal only for this
        // event. The next product action gets an independent best-effort send.
        let _ = runtime.block_on(session.event(name).data(data).send());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(debug_assertions)]
    #[test]
    fn debug_builds_never_enable_analytics() {
        assert!(!analytics_available());
    }

    #[test]
    fn sharing_preference_gates_events_before_the_queue() {
        let (events, receiver) = sync_channel(1);
        let analytics = Analytics {
            events,
            enabled: Arc::new(AtomicBool::new(false)),
            available: true,
        };

        analytics.track(Event::ProjectAdded);
        assert!(receiver.try_recv().is_err());

        analytics.set_enabled(true);
        analytics.track(Event::ProjectAdded);
        assert!(receiver.try_recv().is_ok());

        analytics.set_enabled(false);
        analytics.track(Event::ProjectAdded);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn turn_events_expose_only_coarse_product_metadata() {
        let (name, data) = Event::TurnSubmitted {
            provider: "codex",
            model: "gpt-5".into(),
            turn_number: 2,
            workspace: "worktree",
            projectless: false,
            attachment_count: 1,
            has_input: true,
        }
        .into_track();

        assert_eq!(name, "provider.turn.sent");
        assert_eq!(data["provider"], "codex");
        assert_eq!(data["model"], "gpt-5");
        assert_eq!(data["workspace"], "worktree");
        assert_eq!(data["clientType"], "desktop");
        assert!(data.get("prompt").is_none());
        assert!(data.get("project_path").is_none());
        assert!(data.get("user_id").is_none());
    }

    #[test]
    fn lifecycle_events_expose_only_coarse_product_metadata() {
        let events = [
            Event::TaskCreated {
                provider: "claude",
                workspace: "local",
                projectless: true,
                origin: "external",
            },
            Event::ProviderSetupFinished {
                provider: "codex",
                outcome: "installed",
            },
            Event::RemoteConnectFinished {
                transport: "ssh",
                outcome: "connected",
            },
            Event::AutomationRunFinished {
                trigger: "scheduled",
                outcome: "failed",
                duration_seconds: Some(42),
            },
            Event::GitActionFinished {
                action: "land",
                outcome: "conflict",
            },
            Event::TransferFinished {
                direction: "incoming",
                outcome: "done",
            },
            Event::DaemonRecovery {
                cause: "unexpected_exit",
                outcome: "recovered",
                sessions_resumed: 2,
            },
        ];

        for event in events {
            let (_, data) = event.into_track();
            // Names, paths, ids, hosts, and content never ride along — only
            // the declared product vocabulary.
            for key in [
                "session_id",
                "task_id",
                "run_id",
                "project_id",
                "project_path",
                "peer_id",
                "node_id",
                "host",
                "address",
                "name",
                "title",
                "prompt",
                "error",
                "user_id",
            ] {
                assert!(data.get(key).is_none(), "event leaked {key}: {data}");
            }
        }
    }
}
