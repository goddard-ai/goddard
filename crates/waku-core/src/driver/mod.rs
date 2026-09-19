//! Local provider runtime owned by `goddard-daemon`.

mod acp;
mod activity;
mod amp;
mod claude;
mod codex;
mod computer_use;
mod copilot;
mod deepseek;
mod muse;
mod opencode;
mod opencode2;
mod opencode2_computer_use;
mod pi;
mod support;
mod title_refresh;

pub(crate) use acp::{catalog_agent, discover_devin_models_via_acp};

use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{Receiver, SendError, Sender, unbounded};

use crate::computer_use::ComputerToolRequest;
use crate::model::{
    BackgroundWorkKey, DriverEvent, GoalOperation, MessageAttachment, ProviderKind,
    ProviderResumeCursor, RuntimeMode, UserInputAnswer,
};

/// Provider events remain synchronous to send from reader threads, while the
/// bounded wake channel lets the UI sleep until at least one event is ready.
/// Multiple provider writes coalesce into one wake without ever blocking the
/// provider or dropping the events themselves.
#[derive(Clone)]
pub struct DriverEventSender {
    events: Sender<DriverEvent>,
    wake: smol::channel::Sender<()>,
}

impl DriverEventSender {
    pub fn send(&self, event: DriverEvent) -> Result<(), SendError<DriverEvent>> {
        self.events.send(event)?;
        let _ = self.wake.try_send(());
        Ok(())
    }
}

pub(crate) trait DriverEventSink {
    fn send(&self, event: DriverEvent) -> Result<(), SendError<DriverEvent>>;
}

impl DriverEventSink for DriverEventSender {
    fn send(&self, event: DriverEvent) -> Result<(), SendError<DriverEvent>> {
        DriverEventSender::send(self, event)
    }
}

#[cfg(test)]
impl DriverEventSink for Sender<DriverEvent> {
    fn send(&self, event: DriverEvent) -> Result<(), SendError<DriverEvent>> {
        Sender::send(self, event)
    }
}

pub fn event_channel(
    wake: smol::channel::Sender<()>,
) -> (DriverEventSender, Receiver<DriverEvent>) {
    let (events, receiver) = unbounded();
    (DriverEventSender { events, wake }, receiver)
}

#[cfg(test)]
pub(crate) fn test_event_channel() -> (DriverEventSender, Receiver<DriverEvent>) {
    let (wake, _wakes) = smol::channel::bounded(1);
    event_channel(wake)
}

#[derive(Clone)]
pub struct DriverHandle {
    inner: Arc<dyn DriverControl>,
}

impl DriverHandle {
    pub fn from_control(control: Arc<dyn DriverControl>) -> Self {
        Self { inner: control }
    }

    pub fn prompt(&self, prompt: String) {
        self.inner.prompt(prompt);
    }

    pub fn prompt_with_attachments(&self, prompt: String, attachments: Vec<MessageAttachment>) {
        self.inner.prompt_with_attachments(prompt, attachments);
    }

    /// Whether this transport can inject a user message into the currently
    /// running turn (steering) instead of starting a new one.
    pub fn supports_steer(&self) -> bool {
        self.inner.supports_steer()
    }

    pub fn steer(&self, prompt: String) {
        self.inner.steer(prompt);
    }

    pub fn cancel(&self) {
        self.inner.cancel();
    }

    pub fn cancel_computer_use(&self) {
        self.inner.cancel_computer_use();
    }

    pub fn refresh_background_work(&self) {
        self.inner.refresh_background_work();
    }

    pub fn stop_background_work(&self, key: BackgroundWorkKey, control_id: String) {
        self.inner.stop_background_work(key, control_id);
    }

    pub fn respond(&self, request_id: String, option_id: String) {
        self.inner.respond(request_id, option_id);
    }

    pub fn respond_user_input(&self, request_id: String, answers: Vec<UserInputAnswer>) {
        self.inner.respond_user_input(request_id, answers);
    }

    /// Whether the transport can settle a user-input request without
    /// structured answers — the clarify and dismiss affordances.
    pub fn supports_user_input_actions(&self) -> bool {
        self.inner.supports_user_input_actions()
    }

    pub fn clarify_user_input(&self, request_id: String, content: String) {
        self.inner.clarify_user_input(request_id, content);
    }

    pub fn cancel_user_input(&self, request_id: String) {
        self.inner.cancel_user_input(request_id);
    }

    /// Read or mutate the provider-persisted thread goal. Outcomes arrive
    /// asynchronously as `DriverEvent::GoalUpdated` or `DriverEvent::Error`.
    pub fn goal(&self, operation: GoalOperation) {
        self.inner.goal(operation);
    }

    /// Ask the provider to compact the session's context. Admission,
    /// progress, and the outcome arrive asynchronously through driver
    /// events — a compaction activity card, a turn settle, or
    /// `DriverEvent::Error`.
    pub fn compact(&self) {
        self.inner.compact();
    }

    pub fn run_computer_tool(&self, request: ComputerToolRequest) {
        self.inner.run_computer_tool(request);
    }

    pub fn reject_computer_tool(&self, request: ComputerToolRequest, reason: String) {
        self.inner.reject_computer_tool(request, reason);
    }

    pub fn apply_options(&self, options: SessionOptions) -> bool {
        self.inner.apply_options(options)
    }

    pub fn rollback(&self, turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        self.inner.rollback(turns)
    }

    pub fn fork(&self, turns_to_remove: usize) -> anyhow::Result<ProviderResumeCursor> {
        self.inner.fork(turns_to_remove)
    }
}

pub trait DriverControl: Send + Sync {
    fn prompt(&self, prompt: String);
    /// `prompt` already carries the attachments' `@`-mention text; transports
    /// with a native attachment channel send them structurally too, and the
    /// rest fall back to the mention text alone.
    fn prompt_with_attachments(&self, prompt: String, _attachments: Vec<MessageAttachment>) {
        self.prompt(prompt);
    }
    fn supports_steer(&self) -> bool {
        false
    }
    /// Deliver a steering message to the running turn. Implementations report
    /// the outcome asynchronously through `DriverEvent::SteerAccepted` or
    /// `DriverEvent::SteerRejected`.
    fn steer(&self, _prompt: String) {}
    fn cancel(&self);
    fn cancel_computer_use(&self) {}
    fn refresh_background_work(&self) {}
    fn stop_background_work(&self, _key: BackgroundWorkKey, _control_id: String) {}
    fn respond(&self, request_id: String, option_id: String);
    fn respond_user_input(&self, _request_id: String, _answers: Vec<UserInputAnswer>) {}
    /// Whether clarify/dismiss actions should be offered on user-input
    /// prompts. Off by default — a transport without the semantics would
    /// show dead buttons.
    fn supports_user_input_actions(&self) -> bool {
        false
    }
    fn clarify_user_input(&self, _request_id: String, _content: String) {}
    fn cancel_user_input(&self, _request_id: String) {}
    /// Providers without persisted goals ignore the request; the UI only
    /// offers goal controls where the provider reports one.
    fn goal(&self, _operation: GoalOperation) {}
    /// Ask the provider to compact the session's context. The default sends
    /// the provider's own `/compact` command text, reusing each transport's
    /// existing slash-command routing (registry commands, harness commands,
    /// stream-json user messages). Transports with a dedicated compact RPC
    /// override it.
    fn compact(&self) {
        self.prompt("/compact".to_owned());
    }
    fn run_computer_tool(&self, _request: ComputerToolRequest) {}
    fn reject_computer_tool(&self, _request: ComputerToolRequest, _reason: String) {}
    /// Applies changed turn options to the live session, returning whether the
    /// transport could do it without being restarted. A `false` answer is the
    /// driver asking to be torn down and recreated with the new options.
    fn apply_options(&self, _options: SessionOptions) -> bool {
        false
    }
    fn rollback(&self, turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>>;
    fn fork(&self, _turns_to_remove: usize) -> anyhow::Result<ProviderResumeCursor> {
        anyhow::bail!("conversation forking is not supported by this provider transport")
    }
}

pub struct DriverStartOptions {
    pub binary: PathBuf,
    pub cwd: PathBuf,
    pub mode: RuntimeMode,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub context_window: Option<String>,
    pub agent_preset: Option<String>,
    pub computer_use_enabled: bool,
    /// The scoped agent surface for this launch: the per-session token,
    /// daemon address, and `goddard-agent` CLI location. The daemon fills this
    /// in when `agent_tools_enabled` is on; it never crosses the wire, so no
    /// client can mint itself a credential by setting it.
    pub agent: Option<crate::agent::AgentLaunchEnv>,
    /// Named subagent definitions the driver injects at launch through its
    /// harness's own mechanism. Launch-time only — no transport can
    /// re-inject mid-session, which is why this is not a `SessionOptions`
    /// field.
    pub subagents: Option<waku_protocol::model::SubagentSpec>,
    pub provider_cursor: Option<ProviderResumeCursor>,
    /// The configured evaluation backend, snapshotted at session start.
    /// `Auto`-mode permission requests for providers without their own
    /// reviewer route through it; `None` keeps the ask-the-user fallback.
    /// Daemon-owned — never crosses the wire.
    pub eval: Option<waku_protocol::eval::EvalSettings>,
}

/// The subset of `DriverStartOptions` a user can change without starting a new
/// task. Transports that carry these per turn can absorb a change in place;
/// the rest have to be restarted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionOptions {
    pub mode: RuntimeMode,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub context_window: Option<String>,
}

pub(crate) fn start_local(
    provider: ProviderKind,
    options: DriverStartOptions,
    events: DriverEventSender,
) -> anyhow::Result<DriverHandle> {
    let inner: Arc<dyn DriverControl> = match provider {
        // Antigravity has no driver: its sessions are the CLI's own TUI
        // running in a client-owned terminal, so there is nothing for the
        // daemon to supervise. Reaching this arm means a client asked the
        // daemon to start one anyway.
        ProviderKind::Antigravity => {
            anyhow::bail!("Antigravity sessions are terminal-backed and have no daemon driver")
        }
        ProviderKind::Codex => Arc::new(codex::CodexDriver::start(options, events)?),
        ProviderKind::Pi => Arc::new(pi::PiDriver::start(pi::PiFlavor::Pi, options, events)?),
        ProviderKind::OhMyPi => {
            Arc::new(pi::PiDriver::start(pi::PiFlavor::OhMyPi, options, events)?)
        }
        // Cursor, Devin, Fx, Grok, Kimi Code, Droid, and Goose all serve a
        // long-lived ACP session, which is the only way their Supervised mode
        // can actually ask the user rather than silently forcing or denying.
        ProviderKind::Cursor
        | ProviderKind::Devin
        | ProviderKind::Fx
        | ProviderKind::Grok
        | ProviderKind::Kimi
        | ProviderKind::Droid
        | ProviderKind::Goose => Arc::new(acp::AcpDriver::start(provider, options, events)?),
        ProviderKind::DeepSeek => Arc::new(deepseek::DeepSeekDriver::start(options, events)?),
        // OpenCode's own server is its real API, and it is what exposes
        // interactive permission requests.
        ProviderKind::OpenCode => Arc::new(opencode::OpenCodeDriver::start(options, events)?),
        // OpenCode 2 is not a per-workspace server: one adopted background
        // service carries every workspace, and every Goddard task rides its one
        // event stream.
        ProviderKind::OpenCode2 => Arc::new(opencode2::OpenCode2Driver::start(options, events)?),
        // Muse is the same shape but Goddard owns the host: one `muse serve`
        // multiplexes every session's MSP view over a single stdio connection.
        ProviderKind::Muse => Arc::new(muse::MuseDriver::start(options, events)?),
        // Claude serves a realtime stream of user messages on stdin — the same
        // transport the Agent SDK drives — which is what lets its Supervised
        // mode ask rather than decide alone.
        ProviderKind::Claude => Arc::new(claude::ClaudeDriver::start(options, events)?),
        // Amp reads newline-delimited user messages on stdin and stays alive
        // until stdin closes, so it too serves the whole conversation.
        ProviderKind::Amp => Arc::new(amp::AmpDriver::start(options, events)?),
        // Copilot's official SDK owns the CLI's server-mode lifecycle and its
        // JSON-RPC session; the driver bridges it onto a dedicated Tokio
        // runtime since the SDK's process and transport code is Tokio-native.
        ProviderKind::Copilot => Arc::new(copilot::CopilotDriver::start(options, events)?),
    };
    Ok(DriverHandle { inner })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_events_coalesce_wakes_without_dropping_payloads() {
        let (wake, wakes) = smol::channel::bounded(1);
        let (events, received) = event_channel(wake);

        events.send(DriverEvent::TextDelta("one".into())).unwrap();
        events.send(DriverEvent::TextDelta("two".into())).unwrap();

        assert_eq!(wakes.try_recv(), Ok(()));
        assert!(matches!(
            wakes.try_recv(),
            Err(smol::channel::TryRecvError::Empty)
        ));
        assert!(matches!(received.try_recv(), Ok(DriverEvent::TextDelta(text)) if text == "one"));
        assert!(matches!(received.try_recv(), Ok(DriverEvent::TextDelta(text)) if text == "two"));
    }
}
