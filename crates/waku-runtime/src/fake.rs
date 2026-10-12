//! The deterministic provider harness.
//!
//! `FakeFactory` hands each admitted session a scripted `FakeAdapter`.
//! Every provider interaction the host makes is observable from the test:
//! what was dispatched, how many times, which outcomes were scripted, and
//! what the provider emitted. Events sit in the bounded session channel
//! until `RuntimeHost::pump` drains them — that call is the deterministic
//! barrier that replaces sleeps and timing.
//!
//! `ManualClock` gives retry-window expiry and clock-rollback tests the
//! same determinism.

use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::adapter::{
    AdapterFactory, AdapterHandle, CancelOutcome, DispatchOutcome, OfflineMatrix, SessionContext,
    SpawnError, Termination,
};
use crate::contract::{CanonicalAction, ProviderEvent};
use crate::host::Clock;

/// Deterministic clock. `now_ms` is in epoch milliseconds.
#[derive(Clone)]
pub struct ManualClock {
    now: Arc<AtomicU64>,
}

impl ManualClock {
    pub fn new(now: u64) -> Self {
        Self {
            now: Arc::new(AtomicU64::new(now)),
        }
    }

    /// Move the clock backwards or forwards. The host's persisted time
    /// floor decides what this is allowed to mean.
    pub fn set(&self, ms: u64) {
        self.now.store(ms, Ordering::SeqCst);
    }

    pub fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

/// Spawns one scripted adapter per admission and keeps a handle to it.
/// Cloning shares the spawned-adapter registry.
#[derive(Clone, Default)]
pub struct FakeFactory {
    spawned: Arc<Mutex<Vec<FakeAdapter>>>,
    next_spawn_fault: Arc<Mutex<Option<SpawnError>>>,
}

impl FakeFactory {
    pub fn new() -> Self {
        Self::default()
    }

    /// All adapters spawned so far, in admission order.
    pub fn adapters(&self) -> Vec<FakeAdapter> {
        self.spawned.lock().unwrap().clone()
    }

    /// The adapter bound to a session, if it has been admitted.
    pub fn for_session(&self, session_id: Uuid) -> Option<FakeAdapter> {
        self.spawned
            .lock()
            .unwrap()
            .iter()
            .find(|a| a.session_id() == session_id)
            .cloned()
    }

    /// Make the next `spawn` fail — admission refusal is a recorded
    /// disposition, not a crash.
    pub fn fail_next_spawn(&self, detail: &str) {
        *self.next_spawn_fault.lock().unwrap() = Some(SpawnError(detail.to_string()));
    }
}

impl AdapterFactory for FakeFactory {
    fn spawn(&self, ctx: SessionContext) -> Result<Box<dyn AdapterHandle>, SpawnError> {
        if let Some(fault) = self.next_spawn_fault.lock().unwrap().take() {
            return Err(fault);
        }
        let adapter = FakeAdapter::new(ctx);
        self.spawned.lock().unwrap().push(adapter.clone());
        Ok(Box::new(adapter))
    }
}

/// A scripted provider. The host sees only the `AdapterHandle` surface;
/// tests drive the provider's side — emitted events, scripted outcomes,
/// simulated death — through this shared handle.
#[derive(Clone)]
pub struct FakeAdapter {
    core: Arc<Mutex<FakeCore>>,
}

struct FakeCore {
    session_id: Uuid,
    generation: u64,
    sink: std::sync::mpsc::SyncSender<ProviderEvent>,
    /// Every action the host dispatched, verbatim. The exact count is the
    /// proof that accepted work was never duplicated.
    dispatched: Vec<CanonicalAction>,
    cancels: Vec<Uuid>,
    terminates: usize,
    dispatch_script: VecDeque<DispatchOutcome>,
    cancel_script: VecDeque<CancelOutcome>,
    terminate_script: VecDeque<Termination>,
    matrix: OfflineMatrix,
    alive: bool,
}

impl FakeAdapter {
    fn new(ctx: SessionContext) -> Self {
        Self {
            core: Arc::new(Mutex::new(FakeCore {
                session_id: ctx.session_id,
                generation: ctx.generation,
                sink: ctx.sink,
                dispatched: Vec::new(),
                cancels: Vec::new(),
                terminates: 0,
                dispatch_script: VecDeque::new(),
                cancel_script: VecDeque::new(),
                terminate_script: VecDeque::new(),
                matrix: OfflineMatrix::default(),
                alive: true,
            })),
        }
    }

    pub fn session_id(&self) -> Uuid {
        self.core.lock().unwrap().session_id
    }

    /// The generation this adapter owns.
    pub fn generation(&self) -> u64 {
        self.core.lock().unwrap().generation
    }

    /// Emit a provider event into the session's bounded intake. It becomes
    /// durable only when the host pumps — the explicit barrier.
    pub fn emit(&self, event: ProviderEvent) {
        let core = self.core.lock().unwrap();
        // If the intake is full this blocks the provider, which is exactly
        // what a pausable provider does. Tests that need a full channel can
        // fill it deterministically instead of racing.
        let _ = core.sink.send(event);
    }

    /// Offer a live marker as a real adapter would at attach time.
    pub fn emit_live(&self) {
        self.emit(ProviderEvent::SessionLive);
    }

    pub fn emit_delta(&self, turn_id: Uuid, text: &str) {
        self.emit(ProviderEvent::TurnDelta {
            turn_id,
            delta: json!({ "text": text }),
        });
    }

    pub fn emit_completed(&self, turn_id: Uuid) {
        self.emit(ProviderEvent::TurnCompleted {
            turn_id,
            result: json!({ "status": "done" }),
        });
    }

    /// Script the outcome of the next host dispatch. Unscripted dispatches
    /// report `Accepted`.
    pub fn script_dispatch(&self, outcome: DispatchOutcome) {
        self.core.lock().unwrap().dispatch_script.push_back(outcome);
    }

    /// Script the next cancellation outcome. Unscripted cancels confirm.
    pub fn script_cancel(&self, outcome: CancelOutcome) {
        self.core.lock().unwrap().cancel_script.push_back(outcome);
    }

    /// Script the next termination outcome. Unscripted terminates confirm.
    pub fn script_terminate(&self, outcome: Termination) {
        self.core
            .lock()
            .unwrap()
            .terminate_script
            .push_back(outcome);
    }

    /// Set the declared offline authority matrix.
    pub fn set_matrix(&self, matrix: OfflineMatrix) {
        self.core.lock().unwrap().matrix = matrix;
    }

    /// Everything the host dispatched, in order.
    pub fn dispatched(&self) -> Vec<CanonicalAction> {
        self.core.lock().unwrap().dispatched.clone()
    }

    pub fn dispatch_count(&self) -> usize {
        self.core.lock().unwrap().dispatched.len()
    }

    /// Turn ids the host asked to cancel.
    pub fn cancels(&self) -> Vec<Uuid> {
        self.core.lock().unwrap().cancels.clone()
    }

    pub fn terminate_count(&self) -> usize {
        self.core.lock().unwrap().terminates
    }

    /// Simulate provider death. Dispatch on a dead provider reports
    /// `Unknown` — the honest position when a transport dies mid-flight.
    pub fn kill(&self) {
        self.core.lock().unwrap().alive = false;
    }
}

impl AdapterHandle for FakeAdapter {
    fn dispatch(&mut self, action: &CanonicalAction) -> DispatchOutcome {
        let mut core = self.core.lock().unwrap();
        core.dispatched.push(action.clone());
        if !core.alive {
            return DispatchOutcome::Unknown {
                detail: "provider transport is dead".into(),
            };
        }
        core.dispatch_script
            .pop_front()
            .unwrap_or(DispatchOutcome::Accepted)
    }

    fn cancel(&mut self, turn_id: Uuid) -> CancelOutcome {
        let mut core = self.core.lock().unwrap();
        core.cancels.push(turn_id);
        if !core.alive {
            return CancelOutcome::Unknown {
                detail: "provider transport is dead".into(),
            };
        }
        core.cancel_script
            .pop_front()
            .unwrap_or(CancelOutcome::Confirmed)
    }

    fn terminate(&mut self) -> Termination {
        let mut core = self.core.lock().unwrap();
        core.terminates += 1;
        if !core.alive {
            // A dead provider's termination is trivially confirmed — the
            // interesting case is a live provider that cannot prove it.
            return Termination::Confirmed;
        }
        core.terminate_script.pop_front().unwrap_or_else(|| {
            core.alive = false;
            Termination::Confirmed
        })
    }

    fn alive(&self) -> bool {
        self.core.lock().unwrap().alive
    }

    fn offline_matrix(&self) -> OfflineMatrix {
        self.core.lock().unwrap().matrix.clone()
    }
}

/// Convenience builder for a submit action the way a daemon would send it.
pub fn submit(turn_id: Uuid, delivery_id: Uuid, text: &str) -> CanonicalAction {
    CanonicalAction::Submit {
        delivery_id,
        turn_id,
        input: json!({ "prompt": text }),
    }
}

/// A session's admission action.
pub fn admit(session_id: Uuid) -> CanonicalAction {
    CanonicalAction::AdmitSession {
        session_id,
        adapter: "fake".into(),
        launch: Value::Null,
        requested: Default::default(),
    }
}
