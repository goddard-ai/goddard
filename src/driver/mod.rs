//! Desktop proxy for the provider runtime owned by `goddard-daemon`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::computer_use::ComputerToolRequest;
use crate::model::{
    BackgroundWorkKey, DriverEvent, MessageAttachment, ProviderKind, ProviderResumeCursor,
    RuntimeEventCursor,
};
use crossbeam_channel::{Sender, bounded, select};
use parking_lot::Mutex;

pub use waku_client::driver::{
    DriverControl, DriverEventSender, DriverHandle, DriverStartOptions, SessionOptions,
    event_channel,
};

pub(crate) fn start_remote(
    daemon: waku_client::DaemonSupervisor,
    session_id: uuid::Uuid,
    provider: ProviderKind,
    options: DriverStartOptions,
    events: DriverEventSender,
) -> anyhow::Result<DriverHandle> {
    let client = daemon.client();
    let runtime_id = uuid::Uuid::new_v4();
    let command = waku_client::Command::Start {
        options: waku_client::WireDriverStartOptions {
            provider: waku_client::encode_enum(provider)?,
            binary: options.binary,
            cwd: options.cwd,
            mode: waku_client::encode_enum(options.mode)?,
            model: options.model,
            reasoning_effort: options.reasoning_effort,
            service_tier: options.service_tier,
            context_window: options.context_window,
            agent_preset: options.agent_preset,
            computer_use_enabled: options.computer_use_enabled,
            read_own_transcript: options.read_own_transcript,
            provider_cursor: options
                .provider_cursor
                .map(serde_json::to_value)
                .transpose()?,
        },
    };
    let (supports_steer, supports_user_input_actions) =
        match client.request(session_id, runtime_id, command) {
            Ok(waku_client::ResponsePayload::Started {
                supports_steer,
                supports_user_input_actions,
            }) => (supports_steer, supports_user_input_actions),
            Ok(_) => anyhow::bail!("Goddard daemon returned an invalid start response"),
            Err(error) => return Err(error),
        };
    connect_remote(
        daemon,
        client,
        session_id,
        runtime_id,
        supports_steer,
        supports_user_input_actions,
        None,
        events,
    )
}

pub(crate) fn attach_remote(
    daemon: waku_client::DaemonSupervisor,
    client: waku_client::DaemonClient,
    session_id: uuid::Uuid,
    runtime_id: uuid::Uuid,
    supports_steer: bool,
    supports_user_input_actions: bool,
    replay_cursor: Option<RuntimeEventCursor>,
    events: DriverEventSender,
) -> anyhow::Result<DriverHandle> {
    connect_remote(
        daemon,
        client,
        session_id,
        runtime_id,
        supports_steer,
        supports_user_input_actions,
        replay_cursor,
        events,
    )
}

fn connect_remote(
    daemon: waku_client::DaemonSupervisor,
    initial_client: waku_client::DaemonClient,
    session_id: uuid::Uuid,
    runtime_id: uuid::Uuid,
    supports_steer: bool,
    supports_user_input_actions: bool,
    replay_cursor: Option<RuntimeEventCursor>,
    events: DriverEventSender,
) -> anyhow::Result<DriverHandle> {
    let client_updates = daemon.subscribe_clients();
    let active_client = Arc::new(Mutex::new(initial_client.clone()));
    let forwarding_client = active_client.clone();
    let closed = Arc::new(AtomicBool::new(false));
    let forwarding_closed = closed.clone();
    let (shutdown, forwarding_shutdown) = bounded(1);
    let forwarding_events = events.clone();
    let thread_initial_client = initial_client.clone();
    let spawn = std::thread::Builder::new()
        .name(format!("goddard-daemon-session-{session_id}"))
        .spawn(move || {
            let mut client = thread_initial_client;
            let mut remote_events = client.subscribe(session_id, runtime_id);
            loop {
                let disconnected = loop {
                    select! {
                        recv(forwarding_shutdown) -> _ => return,
                        recv(remote_events) -> sequenced => {
                            let Ok(sequenced) = sequenced else {
                                break true;
                            };
                            if replay_cursor.is_some_and(|cursor| {
                                cursor.runtime_id == sequenced.runtime_id
                                    && cursor.epoch == sequenced.epoch
                                    && cursor.sequence >= sequenced.sequence
                            }) {
                                continue;
                            }
                            let cursor = RuntimeEventCursor {
                                runtime_id: sequenced.runtime_id,
                                epoch: sequenced.epoch,
                                sequence: sequenced.sequence,
                            };
                            let event = match waku_client::event_from_wire(sequenced.event) {
                                Ok(event) => event,
                                Err(error) => DriverEvent::Error(format!(
                                    "Goddard daemon sent an invalid event: {error}"
                                )),
                            };
                            let process_exited = matches!(&event, DriverEvent::ProcessExited);
                            if forwarding_events.send(event).is_err()
                                || forwarding_events
                                    .send(DriverEvent::RuntimeEventCursorAdvanced(cursor))
                                    .is_err()
                            {
                                return;
                            }
                            if process_exited {
                                break false;
                            }
                        }
                    }
                };
                client.unsubscribe(session_id, runtime_id);
                if !disconnected || forwarding_closed.load(Ordering::Acquire) {
                    break;
                }

                // A closed WebSocket says nothing about the provider process.
                // Wait for the supervisor's next connection, then ask that
                // daemon whether this exact runtime survived before changing
                // the task status.
                let replacement = loop {
                    select! {
                        recv(forwarding_shutdown) -> _ => return,
                        recv(client_updates) -> replacement => {
                            let Ok(replacement) = replacement else {
                                return;
                            };
                            if !client.same_connection(&replacement) {
                                break replacement;
                            }
                        }
                    }
                };
                let attached = replacement.request(
                    session_id,
                    uuid::Uuid::nil(),
                    waku_client::Command::AttachSession,
                );
                match attached {
                    Ok(waku_client::ResponsePayload::SessionRuntime {
                        runtime_id: Some(attached_runtime_id),
                        ..
                    }) if attached_runtime_id == runtime_id => {
                        *forwarding_client.lock() = replacement.clone();
                        client = replacement;
                        remote_events = client.subscribe(session_id, runtime_id);
                    }
                    Ok(waku_client::ResponsePayload::SessionRuntime { .. }) => {
                        let _ = forwarding_events.send(DriverEvent::RuntimeLost);
                        break;
                    }
                    Ok(_) => {
                        let _ = forwarding_events.send(DriverEvent::Error(
                            "Goddard daemon returned an invalid runtime attachment response".into(),
                        ));
                        break;
                    }
                    Err(_) => {
                        // This replacement also disappeared. Its client
                        // channel will be followed by another supervisor
                        // publication; keep the task live until one can answer
                        // authoritatively.
                        client = replacement;
                    }
                }
            }
        });
    if let Err(error) = spawn {
        initial_client.unsubscribe(session_id, runtime_id);
        return Err(error.into());
    }
    Ok(DriverHandle::from_control(Arc::new(RemoteDriverControl {
        client: active_client,
        session_id,
        runtime_id,
        supports_steer,
        supports_user_input_actions,
        events,
        closed,
        shutdown,
    })))
}

struct RemoteDriverControl {
    client: Arc<Mutex<waku_client::DaemonClient>>,
    session_id: uuid::Uuid,
    runtime_id: uuid::Uuid,
    supports_steer: bool,
    supports_user_input_actions: bool,
    events: DriverEventSender,
    closed: Arc<AtomicBool>,
    shutdown: Sender<()>,
}

// A notify that lands mid-restart used to fail instantly even though the
// supervisor was about to publish a replacement client. The send itself
// stays fire-and-forget; only a send that found the daemon down retries,
// off the caller's thread, until the replacement arrives or a short budget
// expires.
const REPLACEMENT_WAIT: Duration = Duration::from_secs(2);
const REPLACEMENT_POLL: Duration = Duration::from_millis(50);

impl RemoteDriverControl {
    fn notify(&self, command: waku_client::Command) {
        let client = self.client.lock().clone();
        match client.notify(self.session_id, self.runtime_id, command.clone()) {
            Ok(()) => {}
            Err(_) if client.is_disconnected() => self.retry_with_replacement(command),
            Err(error) => {
                let _ = self.events.send(DriverEvent::Error(format!(
                    "Goddard daemon command failed: {error}"
                )));
            }
        }
    }

    fn retry_with_replacement(&self, command: waku_client::Command) {
        let client_slot = Arc::clone(&self.client);
        let events = self.events.clone();
        let closed = Arc::clone(&self.closed);
        let session_id = self.session_id;
        let runtime_id = self.runtime_id;
        let _ = std::thread::Builder::new()
            .name("goddard-daemon-retry".into())
            .spawn(move || {
                let deadline = Instant::now() + REPLACEMENT_WAIT;
                loop {
                    if closed.load(Ordering::Acquire) {
                        return;
                    }
                    let client = client_slot.lock().clone();
                    if !client.is_disconnected() {
                        match client.notify(session_id, runtime_id, command.clone()) {
                            Ok(()) => return,
                            // The replacement died mid-send; keep waiting for
                            // the next one until the budget expires.
                            Err(_) if client.is_disconnected() => {}
                            Err(error) => {
                                let _ = events.send(DriverEvent::Error(format!(
                                    "Goddard daemon command failed: {error}"
                                )));
                                return;
                            }
                        }
                    }
                    if Instant::now() >= deadline {
                        let _ = events.send(DriverEvent::Error(
                            "the Goddard daemon is unreachable — the command was not delivered"
                                .into(),
                        ));
                        return;
                    }
                    std::thread::sleep(REPLACEMENT_POLL);
                }
            });
    }
}

impl DriverControl for RemoteDriverControl {
    fn prompt(
        &self,
        prompt: String,
        turn_id: Option<uuid::Uuid>,
        message_id: Option<uuid::Uuid>,
        hidden: bool,
        attachments: Vec<waku_protocol::model::MessageAttachment>,
    ) {
        self.notify(waku_client::Command::Prompt {
            prompt,
            turn_id,
            message_id,
            hidden,
            attachments,
        });
    }

    fn supports_steer(&self) -> bool {
        self.supports_steer
    }

    fn steer(&self, prompt: String, hidden: bool) {
        self.notify(waku_client::Command::Steer { prompt, hidden });
    }

    fn cancel(&self) {
        self.notify(waku_client::Command::Cancel);
    }

    fn cancel_computer_use(&self) {
        self.notify(waku_client::Command::CancelComputerUse);
    }

    fn refresh_background_work(&self) {
        self.notify(waku_client::Command::RefreshBackgroundWork);
    }

    fn stop_background_work(&self, key: BackgroundWorkKey, control_id: String) {
        match serde_json::to_value(key) {
            Ok(key) => self.notify(waku_client::Command::StopBackgroundWork { key, control_id }),
            Err(error) => {
                let _ = self.events.send(DriverEvent::Error(format!(
                    "could not encode background-work command: {error}"
                )));
            }
        }
    }

    fn respond(&self, request_id: String, option_id: String) {
        self.notify(waku_client::Command::Respond {
            request_id,
            option_id,
        });
    }

    fn respond_user_input(
        &self,
        request_id: String,
        answers: Vec<waku_protocol::model::UserInputAnswer>,
    ) {
        self.notify(waku_client::Command::RespondUserInput {
            request_id,
            answers,
        });
    }

    fn supports_user_input_actions(&self) -> bool {
        self.supports_user_input_actions
    }

    fn clarify_user_input(&self, request_id: String, content: String) {
        self.notify(waku_client::Command::ClarifyUserInput {
            request_id,
            content,
        });
    }

    fn cancel_user_input(&self, request_id: String) {
        self.notify(waku_client::Command::CancelUserInput { request_id });
    }

    fn goal(&self, operation: waku_protocol::model::GoalOperation) {
        self.notify(waku_client::Command::Goal { operation });
    }

    fn compact(&self) {
        self.notify(waku_client::Command::Compact);
    }

    fn run_computer_tool(&self, request: ComputerToolRequest) {
        self.notify(waku_client::Command::RunComputerTool {
            request: waku_client::WireComputerToolRequest {
                call_id: request.call_id,
                tool: request.tool,
                arguments: request.arguments,
            },
        });
    }

    fn reject_computer_tool(&self, request: ComputerToolRequest, reason: String) {
        self.notify(waku_client::Command::RejectComputerTool {
            request: waku_client::WireComputerToolRequest {
                call_id: request.call_id,
                tool: request.tool,
                arguments: request.arguments,
            },
            reason,
        });
    }

    fn apply_options(&self, options: SessionOptions) -> bool {
        let options = (|| {
            Ok::<_, anyhow::Error>(waku_client::WireSessionOptions {
                mode: waku_client::encode_enum(options.mode)?,
                model: options.model,
                reasoning_effort: options.reasoning_effort,
                service_tier: options.service_tier,
                context_window: options.context_window,
            })
        })();
        let Ok(options) = options else {
            return false;
        };
        let client = self.client.lock().clone();
        matches!(
            client.request(
                self.session_id,
                self.runtime_id,
                waku_client::Command::ApplyOptions { options }
            ),
            Ok(waku_client::ResponsePayload::OptionsApplied { applied: true })
        )
    }

    fn rollback(&self, turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        let client = self.client.lock().clone();
        match client.request(
            self.session_id,
            self.runtime_id,
            waku_client::Command::Rollback { turns },
        )? {
            waku_client::ResponsePayload::Cursor { cursor } => cursor
                .map(serde_json::from_value)
                .transpose()
                .map_err(Into::into),
            _ => anyhow::bail!("Goddard daemon returned an invalid rollback response"),
        }
    }

    fn fork(&self, turns_to_remove: usize) -> anyhow::Result<ProviderResumeCursor> {
        let client = self.client.lock().clone();
        match client.request(
            self.session_id,
            self.runtime_id,
            waku_client::Command::Fork { turns_to_remove },
        )? {
            waku_client::ResponsePayload::Cursor {
                cursor: Some(cursor),
            } => serde_json::from_value(cursor).map_err(Into::into),
            _ => anyhow::bail!("Goddard daemon returned an invalid fork response"),
        }
    }

    fn close(&self) {
        self.notify(waku_client::Command::CloseSession);
    }
}

impl Drop for RemoteDriverControl {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        let _ = self.shutdown.try_send(());
        let client = self.client.lock().clone();
        client.unsubscribe(self.session_id, self.runtime_id);
    }
}

/// A read-only tail of a friend's shared session. The friend's events
/// arrive through our own daemon's broadcast stream — this pump just
/// re-keys them into the runtime event channel, so the transcript applies
/// them like any other driver's. Every control is a no-op: watching is
/// strictly read-only.
pub(crate) fn watch_friend_session(
    client: waku_client::DaemonClient,
    session_id: uuid::Uuid,
    peer_name: String,
    events: DriverEventSender,
) -> anyhow::Result<DriverHandle> {
    let remote_events = client.subscribe_session_events(session_id);
    let closed_events = client.subscribe_friend_session_closed();
    let (shutdown, shutdown_rx) = bounded(1);
    let forwarding = events.clone();
    std::thread::Builder::new()
        .name(format!("goddard-friend-session-{session_id}"))
        .spawn(move || {
            loop {
                select! {
                    recv(shutdown_rx) -> _ => return,
                    recv(remote_events) -> sequenced => {
                        let Ok(sequenced) = sequenced else {
                            return;
                        };
                        let cursor = RuntimeEventCursor {
                            runtime_id: sequenced.runtime_id,
                            epoch: sequenced.epoch,
                            sequence: sequenced.sequence,
                        };
                        let event = match waku_client::event_from_wire(sequenced.event) {
                            Ok(event) => event,
                            Err(error) => DriverEvent::Error(format!(
                                "the friend session sent an invalid event: {error}"
                            )),
                        };
                        // A provider exit on the friend's side doesn't end
                        // the watch — the session-level subscription keeps
                        // streaming once the next runtime starts.
                        if matches!(event, DriverEvent::ProcessExited) {
                            continue;
                        }
                        if forwarding.send(event).is_err()
                            || forwarding
                                .send(DriverEvent::RuntimeEventCursorAdvanced(cursor))
                                .is_err()
                        {
                            return;
                        }
                    }
                    recv(closed_events) -> closed => {
                        let Ok((closed_session, revoked)) = closed else {
                            return;
                        };
                        if closed_session != session_id {
                            continue;
                        }
                        let _ = forwarding.send(DriverEvent::Error(if revoked {
                            format!("{peer_name} stopped sharing this session")
                        } else {
                            "the connection to the friend session ended".to_owned()
                        }));
                        let _ = forwarding.send(DriverEvent::ProcessExited);
                        return;
                    }
                }
            }
        })?;
    Ok(DriverHandle::from_control(Arc::new(FriendWatchControl {
        client,
        session_id,
        shutdown,
    })))
}

struct FriendWatchControl {
    client: waku_client::DaemonClient,
    session_id: uuid::Uuid,
    shutdown: Sender<()>,
}

impl DriverControl for FriendWatchControl {
    fn prompt(
        &self,
        _prompt: String,
        _turn_id: Option<uuid::Uuid>,
        _message_id: Option<uuid::Uuid>,
        _hidden: bool,
        _attachments: Vec<MessageAttachment>,
    ) {
    }

    fn cancel(&self) {}

    fn respond(&self, _request_id: String, _option_id: String) {}

    fn rollback(&self, _turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        anyhow::bail!("a friend's session is read-only")
    }

    fn fork(&self, _turns_to_remove: usize) -> anyhow::Result<ProviderResumeCursor> {
        anyhow::bail!("a friend's session is read-only")
    }

    fn close(&self) {
        let _ = self.shutdown.try_send(());
        let _ = self.client.notify(
            uuid::Uuid::nil(),
            uuid::Uuid::nil(),
            waku_client::Command::UnwatchFriendSession {
                session_id: self.session_id,
            },
        );
    }
}

impl Drop for FriendWatchControl {
    fn drop(&mut self) {
        self.close();
    }
}
