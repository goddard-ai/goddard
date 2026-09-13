//! Scoped agent credentials and the daemon-side prompt queue behind them.
//!
//! When `agent_tools_enabled` is on, the daemon mints one bearer token per
//! provider runtime and hands it to the session's harness through the launch
//! environment (`WAKU_AGENT_TOKEN`, see [`crate::driver`]). The token never
//! leaves daemon memory: it authenticates a WebSocket client exactly like the
//! master token but authorizes only `agentCreateSession` and `agentPrompt`,
//! and it dies with the runtime that carried it.
//!
//! Queue-mode prompts wait here rather than in the session's client-side
//! follow-up queue: delivery order is decided by the daemon, and the daemon
//! is also what marks each accepted prompt with the sending task's id so
//! agent-originated turns stay attributable in the transcript.

use std::collections::{HashMap, VecDeque};

use parking_lot::Mutex;
use subtle::ConstantTimeEq as _;
use uuid::Uuid;

use crate::model::DriverEvent;

/// A prompt an agent submitted, with the sending task it must be attributed
/// to. The same record tracks steer injections awaiting the provider's
/// `steerAccepted` echo so the mirrored message keeps its provenance.
pub struct AgentPrompt {
    pub prompt: String,
    /// The task whose agent sent it. `None` is possible only for requests
    /// made with the master token, which carries no session scope.
    pub sender: Option<Uuid>,
}

/// Live turn bookkeeping the runtime event forwarder maintains per session.
#[derive(Clone, Copy, Default)]
struct AgentTurn {
    /// The provider reported `turnStarted` without a later `turnFinished`.
    open: bool,
    /// The provider is actively working inside the open turn. A parked turn
    /// stays open but idle: it accepts a message immediately.
    working: bool,
}

/// Shared agent surface state. One instance lives on the backend; the runtime
/// event forwarder holds a second reference so it can track turns, drain
/// queues, and attach provenance to echoed steers without round-tripping
/// through the request path.
#[derive(Default)]
pub struct AgentState {
    /// Scoped bearer token → the session owning the runtime it was minted
    /// for. Several tokens can name the same session when a task restarts.
    tokens: Mutex<HashMap<String, Uuid>>,
    /// Queue-mode prompts waiting for the target session to become idle.
    queues: Mutex<HashMap<Uuid, VecDeque<AgentPrompt>>>,
    /// Steer injections in flight, oldest first.
    pending_steers: Mutex<HashMap<Uuid, VecDeque<AgentPrompt>>>,
    turns: Mutex<HashMap<Uuid, AgentTurn>>,
}

impl AgentState {
    /// Mint a credential scoped to one provider session. The value is two
    /// UUIDs of entropy; it is stored, never derived, and never written down.
    pub fn mint(&self, session_id: Uuid) -> String {
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        self.tokens.lock().insert(token.clone(), session_id);
        token
    }

    /// Resolve a presented credential to its owning session. The comparison
    /// is constant-time over every minted token so probes cannot narrow the
    /// space by timing.
    pub fn resolve(&self, token: &str) -> Option<Uuid> {
        let candidate = token.as_bytes();
        self.tokens.lock().iter().find_map(|(known, session)| {
            bool::from(known.as_bytes().ct_eq(candidate)).then_some(*session)
        })
    }

    /// Drop every credential minted for the session's runtimes. Called when a
    /// runtime is closed, replaced, or reported exited — the token is valid
    /// only while the provider process that carries it lives.
    pub fn revoke_session(&self, session_id: Uuid) {
        self.tokens.lock().retain(|_, owner| *owner != session_id);
    }

    /// Forget everything about a session that is gone for good.
    pub fn clear_session(&self, session_id: Uuid) {
        self.revoke_session(session_id);
        self.queues.lock().remove(&session_id);
        self.pending_steers.lock().remove(&session_id);
        self.turns.lock().remove(&session_id);
    }

    /// Forget every credential and queue. Called on daemon shutdown.
    pub fn clear(&self) {
        self.tokens.lock().clear();
        self.queues.lock().clear();
        self.pending_steers.lock().clear();
        self.turns.lock().clear();
    }

    /// Update the session's turn bookkeeping from a runtime event.
    pub fn note_driver_event(&self, session_id: Uuid, event: &DriverEvent) {
        match event {
            DriverEvent::TurnStarted => {
                *self.turns.lock().entry(session_id).or_default() = AgentTurn {
                    open: true,
                    working: true,
                };
            }
            DriverEvent::TurnParked => {
                if let Some(turn) = self.turns.lock().get_mut(&session_id) {
                    turn.working = false;
                }
            }
            DriverEvent::TurnFinished { .. } => {
                if let Some(turn) = self.turns.lock().get_mut(&session_id) {
                    turn.open = false;
                    turn.working = false;
                }
            }
            DriverEvent::ProcessExited => {
                self.turns.lock().remove(&session_id);
                // Steers can no longer be acknowledged; drop them so a
                // restarted runtime does not attribute an unrelated echo.
                self.pending_steers.lock().remove(&session_id);
            }
            DriverEvent::SteerRejected { message, .. } => {
                self.take_pending_steer(session_id, message);
            }
            _ => {}
        }
    }

    /// Whether the session has an open turn — running or parked. Steer-mode
    /// prompts require this.
    pub fn has_open_turn(&self, session_id: Uuid) -> bool {
        self.turns
            .lock()
            .get(&session_id)
            .is_some_and(|turn| turn.open)
    }

    /// Whether the provider is actively working a turn. Queue-mode prompts
    /// wait while this is true; a parked session (open but not working)
    /// accepts the next message right away.
    pub fn is_working(&self, session_id: Uuid) -> bool {
        self.turns
            .lock()
            .get(&session_id)
            .is_some_and(|turn| turn.open && turn.working)
    }

    /// Whether the session has an open turn that is not working — the state
    /// a queue-mode prompt may steer into without cutting a running turn
    /// short.
    pub fn has_parked_turn(&self, session_id: Uuid) -> bool {
        self.turns
            .lock()
            .get(&session_id)
            .is_some_and(|turn| turn.open && !turn.working)
    }

    pub fn enqueue(&self, session_id: Uuid, prompt: AgentPrompt) {
        self.queues
            .lock()
            .entry(session_id)
            .or_default()
            .push_back(prompt);
    }

    /// Pop the next queued prompt in submission order.
    pub fn pop_queued(&self, session_id: Uuid) -> Option<AgentPrompt> {
        let mut queues = self.queues.lock();
        let queue = queues.get_mut(&session_id)?;
        let prompt = queue.pop_front();
        if queue.is_empty() {
            queues.remove(&session_id);
        }
        prompt
    }

    /// Put a popped prompt back at the head of the queue. A turn that
    /// started working mid-drain holds the remaining prompts until it
    /// finishes; submission order is preserved.
    pub fn requeue_front(&self, session_id: Uuid, prompt: AgentPrompt) {
        self.queues
            .lock()
            .entry(session_id)
            .or_default()
            .push_front(prompt);
    }

    /// Remember a steer injection so the provider's `steerAccepted` echo can
    /// be attributed to its sender.
    pub fn record_pending_steer(&self, session_id: Uuid, prompt: AgentPrompt) {
        self.pending_steers
            .lock()
            .entry(session_id)
            .or_default()
            .push_back(prompt);
    }

    /// Resolve the steer the provider just accepted or rejected. Providers
    /// normally echo the transport text; a normalized echo still releases
    /// the oldest pending steer, matching how clients treat it.
    pub fn take_pending_steer(&self, session_id: Uuid, message: &str) -> Option<AgentPrompt> {
        let mut steers = self.pending_steers.lock();
        let pending = steers.get_mut(&session_id)?;
        let index = pending
            .iter()
            .position(|steer| steer.prompt == message)
            .unwrap_or(0);
        let prompt = pending.remove(index);
        if pending.is_empty() {
            steers.remove(&session_id);
        }
        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(text: &str, sender: Option<Uuid>) -> AgentPrompt {
        AgentPrompt {
            prompt: text.to_owned(),
            sender,
        }
    }

    #[test]
    fn a_minted_token_resolves_to_its_owning_session_until_revoked() {
        let state = AgentState::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let first_token = state.mint(first);
        let second_token = state.mint(second);

        assert_eq!(state.resolve(&first_token), Some(first));
        assert_eq!(state.resolve(&second_token), Some(second));
        assert_eq!(state.resolve("not-a-token"), None);

        state.revoke_session(first);
        assert_eq!(state.resolve(&first_token), None);
        assert_eq!(state.resolve(&second_token), Some(second));
    }

    #[test]
    fn queued_prompts_pop_in_submission_order() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        state.enqueue(session, prompt("one", None));
        state.enqueue(session, prompt("two", None));

        assert_eq!(state.pop_queued(session).unwrap().prompt, "one");
        // A requeued prompt returns to the head ahead of later submissions.
        let popped = prompt("one", None);
        state.requeue_front(session, popped);
        state.enqueue(session, prompt("three", None));

        let order: Vec<String> = [
            state.pop_queued(session),
            state.pop_queued(session),
            state.pop_queued(session),
        ]
        .into_iter()
        .flatten()
        .map(|entry| entry.prompt)
        .collect();
        assert_eq!(order, ["one", "two", "three"]);
        assert!(state.pop_queued(session).is_none());
    }

    #[test]
    fn turn_events_decide_where_a_prompt_waits() {
        let state = AgentState::default();
        let session = Uuid::new_v4();

        assert!(!state.has_open_turn(session));
        assert!(!state.is_working(session));
        assert!(!state.has_parked_turn(session));

        state.note_driver_event(session, &DriverEvent::TurnStarted);
        assert!(state.has_open_turn(session));
        assert!(state.is_working(session));
        assert!(!state.has_parked_turn(session));

        state.note_driver_event(session, &DriverEvent::TurnParked);
        assert!(state.has_open_turn(session));
        assert!(!state.is_working(session));
        assert!(state.has_parked_turn(session));

        state.note_driver_event(
            session,
            &DriverEvent::TurnFinished {
                success: true,
                summary: None,
            },
        );
        assert!(!state.has_open_turn(session));
        assert!(!state.is_working(session));
    }

    #[test]
    fn an_accepted_steer_releases_its_sender_provenance() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        let sender = Uuid::new_v4();
        state.record_pending_steer(session, prompt("first", Some(sender)));
        state.record_pending_steer(session, prompt("second", None));

        // An exact echo takes its own entry, not the oldest.
        let taken = state.take_pending_steer(session, "second").unwrap();
        assert_eq!(taken.prompt, "second");
        assert_eq!(taken.sender, None);

        // A normalized echo releases the oldest pending steer.
        let taken = state
            .take_pending_steer(session, "unrecognized echo")
            .unwrap();
        assert_eq!(taken.prompt, "first");
        assert_eq!(taken.sender, Some(sender));
        assert!(state.take_pending_steer(session, "anything").is_none());
    }

    #[test]
    fn process_exit_drops_pending_steers_and_turn_state() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        state.note_driver_event(session, &DriverEvent::TurnStarted);
        state.record_pending_steer(session, prompt("in flight", Some(Uuid::new_v4())));

        state.note_driver_event(session, &DriverEvent::ProcessExited);

        assert!(!state.has_open_turn(session));
        assert!(state.take_pending_steer(session, "in flight").is_none());
    }

    #[test]
    fn clear_session_forgets_credentials_queue_and_turns() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        let token = state.mint(session);
        state.enqueue(session, prompt("held", None));
        state.note_driver_event(session, &DriverEvent::TurnStarted);

        state.clear_session(session);

        assert_eq!(state.resolve(&token), None);
        assert!(state.pop_queued(session).is_none());
        assert!(!state.has_open_turn(session));
    }
}
