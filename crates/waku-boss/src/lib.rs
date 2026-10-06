//! Boss service, policy, and eval runtime, isolated from the daemon host.

pub mod boss;
pub mod boss_context;
pub mod boss_eval;
pub mod boss_rotation;

pub use boss::BossService;

use std::sync::Arc;

/// Host notification hook used when persisted Boss state changes.
pub type TaskNotifier = Arc<dyn Fn() + Send + Sync>;

/// Narrow callback into the daemon for completing or recovering one employee.
pub type FinishEmployee = Arc<dyn Fn(uuid::Uuid) -> anyhow::Result<()> + Send + Sync>;

/// Restart reconciliation callback, separate from ordinary settlement.
pub type RecoverEmployee = Arc<dyn Fn(uuid::Uuid) -> anyhow::Result<()> + Send + Sync>;

/// Read-only task state supplied by the daemon host.
pub type SessionActive = Arc<dyn Fn(uuid::Uuid) -> bool + Send + Sync>;

/// Read-only turn-liveness probe supplied by the daemon host — `true`
/// while the session has an open provider turn (running or parked).
pub type SessionBusy = Arc<dyn Fn(uuid::Uuid) -> bool + Send + Sync>;

/// Daemon-owned session archival used by finalized-plan expiry.
pub type ArchiveSessions = Arc<dyn Fn(&[uuid::Uuid]) -> anyhow::Result<bool> + Send + Sync>;

/// The daemon's registered project list, read on demand so operations like
/// memory-bucket resolution can turn a project name into its canonical path.
pub type ProjectCatalog = Arc<dyn Fn() -> Vec<waku_protocol::model::Project> + Send + Sync>;
