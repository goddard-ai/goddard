//! Observable, daemon-owned repository landing jobs.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::git::{LandOutcome, PullStrategy};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LandingKind {
    Land,
    Submit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LandingState {
    Queued,
    Running,
    Canceled,
    Finished,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LandingResult {
    Land { outcome: LandOutcome },
    Submitted { sha: String },
    Failed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LandingJob {
    pub id: Uuid,
    pub sequence: u64,
    pub queue_id: Uuid,
    pub task_id: Option<Uuid>,
    pub cwd: String,
    pub kind: LandingKind,
    pub base: Option<String>,
    pub strategy: PullStrategy,
    pub state: LandingState,
    /// One-based among waiting jobs; the active job is excluded.
    pub position: Option<u32>,
    pub result: Option<LandingResult>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LandingQueueSnapshot {
    pub epoch: Uuid,
    pub queue_id: Uuid,
    pub revision: u64,
    /// Includes recent terminal jobs so returning clients can reconcile.
    pub jobs: Vec<LandingJob>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CancelLandingOutcome {
    Canceled,
    AlreadyRunning,
    AlreadyFinished,
    NotFound,
}
