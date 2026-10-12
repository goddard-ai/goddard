//! The provider-ownership seam.
//!
//! The runtime is the exclusive owner of provider children and transports.
//! An adapter handle exists only because a session was admitted, speaks only
//! to its own generation, and reports facts — never decisions — through a
//! bounded channel the host drains in order. What the adapter cannot prove
//! (acceptance, cancellation, termination) it reports as unknown; the host
//! turns that into conservative dispositions rather than retries.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::mpsc::SyncSender;
use thiserror::Error;
use uuid::Uuid;

use crate::contract::{CanonicalAction, ProviderEvent, RequestedConfig};

/// Spawn-time context for one provider generation. The sink is bounded so a
/// provider that cannot pause is visible as backpressure instead of growing
/// memory without limit.
pub struct SessionContext {
    pub session_id: Uuid,
    pub generation: u64,
    pub namespace: String,
    pub adapter: String,
    /// Opaque launch material from the admission.
    pub launch: Value,
    pub requested: RequestedConfig,
    /// Ordered intake. Events become durable only when the host drains and
    /// journals them; a bounded channel is the honest pause point for
    /// providers that support it.
    pub sink: SyncSender<ProviderEvent>,
}

/// The host's exclusive handle on one provider incarnation.
pub trait AdapterHandle: Send {
    /// Dispatch a journaled command to the provider. Called only after the
    /// host has persisted dispatch intent, so `Unknown` results can be
    /// held uncertain instead of retried.
    fn dispatch(&mut self, action: &CanonicalAction) -> DispatchOutcome;

    /// Attempt cancellation of one named turn.
    fn cancel(&mut self, turn_id: Uuid) -> CancelOutcome;

    /// Best-effort termination of the owned provider. The result is exactly
    /// what the host may claim; `Unconfirmed` leaves the session visibly
    /// stopping rather than stopped.
    fn terminate(&mut self) -> Termination;

    /// Whether the underlying provider is still reachable, as last observed.
    fn alive(&self) -> bool;

    /// The adapter's declared offline authority matrix — what may still
    /// happen while the daemon is absent.
    fn offline_matrix(&self) -> OfflineMatrix;
}

/// Result of handing a recorded command to the provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// The provider confirmed acceptance of this exact command.
    Accepted,
    /// The provider refused before acceptance.
    Rejected { code: String, detail: String },
    /// The attempt's outcome cannot be proven. The host records this as
    /// uncertain; it is never automatically resent.
    Unknown { detail: String },
}

/// Result of a cancellation attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CancelOutcome {
    Confirmed,
    /// The named turn was not running — an honest no-op.
    NotRunning,
    Failed {
        detail: String,
    },
    /// The attempt's outcome cannot be proven.
    Unknown {
        detail: String,
    },
}

/// Observed result of a termination attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Termination {
    /// The owner provably ended.
    Confirmed,
    /// Termination cannot be proven; the host keeps the session visibly
    /// stopping rather than claiming it stopped.
    Unconfirmed { detail: String },
}

/// Creates adapter handles for admitted sessions. The factory, not the
/// session record, decides how a provider is launched.
pub trait AdapterFactory: Send + Sync {
    fn spawn(&self, ctx: SessionContext) -> Result<Box<dyn AdapterHandle>, SpawnError>;
}

#[derive(Debug, Error)]
#[error("adapter spawn failed: {0}")]
pub struct SpawnError(pub String);

/// Whether the adapter can enforce an authority boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Enforcement {
    Enforced,
    /// Unsupported — disclosed, never represented as enforced.
    Unavailable,
    /// Not yet measured.
    Unverified,
}

/// The provider's native callback/caller deadline behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "bound",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DeadlineBound {
    /// A measured native deadline; callers may not wait longer.
    Bounded { ms: u64 },
    /// The provider does not impose a shorter deadline than the caller's.
    Unbounded,
    /// Not yet established — a shorter native timeout may interrupt turns.
    Unmeasured,
}

/// What cancellation is known to terminate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CancellationLimit {
    /// The owned process and its descendants are provably ended.
    ProcessAndDescendants,
    /// Only the provider's own execution; descendants/remote work may run on.
    ProviderOnly,
    Unverified,
}

/// Each adapter declares what stays possible while the daemon is absent.
/// Human approval of offline native execution relies on these declared
/// limits; unsupported expiry/revocation is disclosed, never implied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineMatrix {
    /// Provider-native operations already allowed by the issued grant that
    /// can continue without the daemon.
    pub native_ops: Vec<String>,
    /// Whether native authority supports expiry or revocation mid-execution.
    pub revocation: Enforcement,
    /// Native callback/caller deadline behavior.
    pub callback_deadline: DeadlineBound,
    /// Whether provider intake can pause under bounded backpressure.
    pub can_pause_intake: bool,
    /// What cancellation is known to terminate.
    pub cancellation: CancellationLimit,
}

impl Default for OfflineMatrix {
    fn default() -> Self {
        Self {
            native_ops: Vec::new(),
            revocation: Enforcement::Unverified,
            callback_deadline: DeadlineBound::Unmeasured,
            can_pause_intake: false,
            cancellation: CancellationLimit::Unverified,
        }
    }
}
