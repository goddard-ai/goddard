//! Advisory inference only: never emit provider compaction events or drive recovery.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::model::{AgentSession, ContextUsage, DriverEvent, ProviderKind, is_context_compaction};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactionHint {
    Possible,
    Likely,
}

#[derive(Clone, Debug)]
struct PromptWait {
    id: Uuid,
    started: Instant,
    usage: Option<ContextUsage>,
    threshold: Duration,
    first_activity: bool,
    delayed: bool,
    likely: bool,
}

/// Process-local observations. Replays, snapshots, and restarts deliberately
/// start without a timing claim; elapsed wall time is not provider evidence.
#[derive(Clone, Debug, Default)]
pub struct CompactionMonitor {
    binding: Option<(ProviderKind, Option<String>, Option<String>)>,
    recent_waits: VecDeque<Duration>,
    usage_valid: bool,
    prompt: Option<PromptWait>,
}

impl CompactionMonitor {
    pub fn hint(&self, now: Instant) -> Option<CompactionHint> {
        let prompt = self.prompt.as_ref()?;
        if prompt.likely {
            Some(CompactionHint::Likely)
        } else if !prompt.first_activity && suspicious(prompt, now) {
            Some(CompactionHint::Possible)
        } else {
            None
        }
    }

    fn observe(&mut self, event: &DriverEvent, usage: Option<ContextUsage>, now: Instant) {
        match event {
            DriverEvent::PromptSubmitted { turn_id, .. } => {
                self.submitted(*turn_id, usage, now);
            }
            DriverEvent::Connected { .. }
            | DriverEvent::ConfigActivated { .. }
            | DriverEvent::TurnCancelled
            | DriverEvent::TurnParked
            | DriverEvent::RuntimeLost
            | DriverEvent::ProcessExited
            | DriverEvent::Error(_)
            | DriverEvent::LocalizedError { .. } => self.prompt = None,
            DriverEvent::RichActivity(item) if is_context_compaction(item) => {
                // Explicit provider evidence owns its existing presentation.
                self.prompt = None;
            }
            DriverEvent::UsageUpdated {
                context_tokens,
                context_window,
            } => {
                if context_tokens.is_some() {
                    self.usage_valid = true;
                }
                if let Some(window) = context_window
                    && self
                        .prompt
                        .as_ref()
                        .and_then(|prompt| prompt.usage)
                        .is_some_and(|before| before.window != Some(*window))
                {
                    self.prompt = None;
                    return;
                }
                let Some(tokens) = context_tokens else {
                    return;
                };
                if let Some(prompt) = &mut self.prompt {
                    prompt.delayed |= suspicious(prompt, now);
                    if let Some(before) = prompt.usage
                        && near_limit(before)
                        && context_window.is_none_or(|window| Some(window) == before.window)
                        && *tokens > 0
                        && before.tokens.saturating_sub(*tokens) >= 8_000
                        && *tokens <= before.tokens.saturating_mul(3) / 4
                        && prompt.delayed
                    {
                        prompt.likely = true;
                    }
                }
            }
            DriverEvent::TextDelta(text) | DriverEvent::ReasoningDelta(text)
                if !text.trim().is_empty() =>
            {
                self.activity(now);
            }
            DriverEvent::Activity { .. }
            | DriverEvent::RichActivity(_)
            | DriverEvent::Permission { .. }
            | DriverEvent::UserInputRequested { .. } => self.activity(now),
            DriverEvent::TurnFinished { .. } => {
                // A likely observation can remain in the usage panel until the
                // next prompt. Never leave a completed turn looking stalled.
                if let Some(prompt) = &mut self.prompt {
                    prompt.first_activity = true;
                }
            }
            _ => {}
        }
    }

    fn submitted(&mut self, turn_id: Uuid, usage: Option<ContextUsage>, now: Instant) {
        // A local submission followed by the daemon's echo is one wait.
        if self
            .prompt
            .as_ref()
            .is_some_and(|prompt| prompt.id == turn_id)
        {
            return;
        }
        let threshold = if self.recent_waits.len() >= 5 {
            let mut samples: Vec<_> = self.recent_waits.iter().copied().collect();
            samples.sort_unstable();
            samples[samples.len() / 2]
                .saturating_mul(3)
                .max(Duration::from_secs(30))
        } else {
            Duration::from_secs(60)
        };
        self.prompt = Some(PromptWait {
            id: turn_id,
            started: now,
            usage: usage.filter(|_| self.usage_valid),
            threshold,
            first_activity: false,
            delayed: false,
            likely: false,
        });
    }

    fn activity(&mut self, now: Instant) {
        if let Some(prompt) = &mut self.prompt
            && !prompt.first_activity
        {
            prompt.delayed |= suspicious(prompt, now);
            prompt.first_activity = true;
            // Don't teach the baseline that suspected compaction is normal.
            if !prompt.delayed {
                if self.recent_waits.len() == 20 {
                    self.recent_waits.pop_front();
                }
                self.recent_waits
                    .push_back(now.saturating_duration_since(prompt.started));
            }
        }
    }
}

fn near_limit(usage: ContextUsage) -> bool {
    usage.window.is_some_and(|window| {
        window > 0 && usage.tokens <= window && usage.tokens >= window.saturating_mul(85) / 100
    })
}

fn suspicious(prompt: &PromptWait, now: Instant) -> bool {
    !prompt.first_activity
        && prompt.usage.is_some_and(near_limit)
        && now.saturating_duration_since(prompt.started) >= prompt.threshold
}

impl AgentSession {
    pub fn compaction_hint(&self, now: Instant) -> Option<CompactionHint> {
        if self.compaction_binding_matches() {
            self.compaction_monitor.hint(now)
        } else {
            None
        }
    }

    fn compaction_binding_matches(&self) -> bool {
        self.compaction_monitor
            .binding
            .as_ref()
            .is_some_and(|(provider, model, effort)| {
                *provider == self.provider
                    && model == &self.model
                    && effort == &self.reasoning_effort
            })
    }

    /// Call before applying the event so usage drops compare against the
    /// dispatch-time occupancy, never cumulative billing or a changed model.
    pub fn observe_compaction(&mut self, event: &DriverEvent, now: Instant) {
        if let DriverEvent::PromptSubmitted { turn_id, .. } = event {
            self.observe_prompt_dispatch(*turn_id, now);
            return;
        }
        self.bind_compaction_monitor();
        self.compaction_monitor
            .observe(event, self.context_usage, now);
    }

    /// Dispatch for daemon-delivered prompts that do not pass through its event recorder.
    pub fn observe_prompt_dispatch(&mut self, turn_id: Uuid, now: Instant) {
        // A report folded into an existing turn is a steer, not a new wait.
        if self
            .active_turn_id()
            .is_some_and(|active| active != turn_id)
        {
            return;
        }
        self.bind_compaction_monitor();
        self.compaction_monitor
            .submitted(turn_id, self.context_usage, now);
    }

    fn bind_compaction_monitor(&mut self) {
        if !self.compaction_binding_matches() {
            let first_binding = self.compaction_monitor.binding.is_none();
            self.compaction_monitor = CompactionMonitor {
                // A changed model must report fresh usage before its next
                // dispatch can be compared against a context window.
                usage_valid: first_binding,
                binding: Some((
                    self.provider,
                    self.model.clone(),
                    self.reasoning_effort.clone(),
                )),
                ..Default::default()
            };
        }
    }
}
