//! Stable agent runtime host — stage 1 of the approved stable-runtime plan.
//!
//! A third, separately surviving owner for provider sessions. The desktop
//! app displays state and the daemon owns product policy; this crate owns
//! provider children, ordered durable event capture, command deduplication,
//! control epochs, and stop fences. Replacing the app or daemon changes the
//! controller, never the provider.
//!
//! The crate deliberately has no `waku-*` dependencies: no localization, UI,
//! product policy, daemon, or CLI source may sit in the stable runtime's
//! transitive build inputs, so ordinary product edits can never rebuild it.
//!
//! - [`contract`] — the narrow typed protocol every owner consumes.
//! - [`adapter`] — the provider-ownership seam adapters implement.
//! - [`journal`] — the append-only durable record log.
//! - [`host`] — `RuntimeHost`: identity, epochs, dedup, fences, budgets.
//! - [`fake`] — the deterministic scripted-provider harness and clock.

pub mod adapter;
pub mod contract;
pub mod fake;
pub mod host;
pub mod journal;

pub use adapter::{
    AdapterFactory, AdapterHandle, CancelOutcome, CancellationLimit, DeadlineBound,
    DispatchOutcome, Enforcement, OfflineMatrix, SessionContext, SpawnError, Termination,
};
pub use contract::*;
pub use fake::{FakeAdapter, FakeFactory, ManualClock};
pub use host::{Budgets, Clock, Credentials, HostOptions, RuntimeHost, SystemClock};
pub use journal::{JournalError, JournalFault};

#[cfg(test)]
mod tests;
