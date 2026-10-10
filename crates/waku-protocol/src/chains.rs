//! Durable linear employee-chain contracts. Execution adapters must not infer
//! success from employee expiry, admission, or a provisional result.
use crate::{
    AgentWorkspace, boss::PersonaPermissions, model::ProviderKind, resources::ResourceSet,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

macro_rules! chain_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
        #[serde(rename_all = "camelCase")]
        pub enum $name { $($variant),+ }
    };
}
chain_enum!(ChainStatus {
    Waiting,
    Queued,
    Starting,
    Working,
    WaitingForEmployee,
    CheckingHandoff,
    Pausing,
    Cancelling,
    Paused,
    Completed,
    Cancelled
});
chain_enum!(StepStatus {
    NotStarted,
    Waiting,
    Queued,
    Starting,
    Working,
    WaitingForEmployee,
    CheckingHandoff,
    Succeeded,
    Failed,
    Cancelled
});
chain_enum!(HandoffBoundary {
    ArtifactsReady,
    CommittedWorkspace,
    Landed
});
chain_enum!(ResultDisposition {
    Success,
    Blocked,
    NeedsGuidance,
    Failed
});
chain_enum!(CheckStatus { Pass, Fail, NotRun });
chain_enum!(ChainControl {
    Pause,
    Resume,
    Retry,
    Cancel
});

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChainStepDefinition {
    pub title: String,
    pub prompt: String,
    pub project_id: Uuid,
    pub persona_id: Option<Uuid>,
    pub provider: ProviderKind,
    /// Concrete selection, never inherited or auto-routed at dispatch.
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub permissions: PersonaPermissions,
    pub resources: ResourceSet,
    pub workspace: AgentWorkspace,
    pub base_revision: Option<String>,
    pub boundary: HandoffBoundary,
    pub required_checks: Vec<String>,
    pub required_artifacts: Vec<String>,
    /// An explicit decision is required for each name; elapsed time is not approval.
    pub required_approvals: Vec<String>,
    /// Manual retry budget; capacity waiting consumes none of it.
    pub max_retries: u32,
    pub plan_id: Option<Uuid>,
    pub item_id: Option<Uuid>,
    pub successor_brief: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChainDefinition {
    pub outcome_id: Uuid,
    pub finishes_outcome: bool,
    /// Ordered steps are the entire dependency graph. Branches and loops have no representation.
    pub steps: Vec<ChainStepDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChainCheck {
    pub name: String,
    pub status: CheckStatus,
    pub evidence: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChainArtifact {
    pub name: String,
    pub reference: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChainCodeEvidence {
    pub workspace: String,
    pub source_revision: String,
    pub committed_revision: Option<String>,
    pub landed_revision: Option<String>,
    pub qa_branch: Option<String>,
    pub submission_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChainResult {
    pub disposition: ResultDisposition,
    pub checks: Vec<ChainCheck>,
    pub artifacts: Vec<ChainArtifact>,
    pub code: Option<ChainCodeEvidence>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChainAttempt {
    pub id: Uuid,
    /// Stable external admission key. Persist before contacting the scheduler.
    pub dispatch_id: Uuid,
    pub definition_version: u64,
    pub employee_id: Option<Uuid>,
    pub status: StepStatus,
    pub starting_revision: Option<String>,
    pub workspace: Option<String>,
    pub execution_released: bool,
    pub workspace_released: bool,
    pub provisional_result: Option<ChainResult>,
    pub terminal: Option<ChainTerminal>,
    pub sealed: Option<ChainHandoff>,
    pub handoff_accepted: bool,
    pub reconciled_retry_revision: Option<String>,
    pub created_at: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChainTerminal {
    pub message: String,
    pub disposition: ResultDisposition,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChainHandoff {
    pub producer_attempt: Uuid,
    pub definition_version: u64,
    /// Preserved byte-for-byte, independently of structured evidence.
    pub terminal_message: String,
    pub result: ChainResult,
    /// References a durable step, never a fabricated future employee.
    pub successor_step: Option<usize>,
    pub accepted: bool,
    pub reason: Option<String>,
    pub sealed_at: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChainStep {
    pub definition: ChainStepDefinition,
    pub status: StepStatus,
    pub attempts: Vec<ChainAttempt>,
    pub approvals: Vec<ChainApproval>,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChainActor {
    Human,
    Daemon,
    Session { session_id: Uuid },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChainApproval {
    pub gate: String,
    pub actor: ChainActor,
    pub approved: bool,
    pub definition_version: u64,
    pub at: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChainHistory {
    pub event_id: Uuid,
    pub actor: ChainActor,
    pub step: usize,
    pub attempt_id: Option<Uuid>,
    pub definition_version: u64,
    pub revision: u64,
    pub event: String,
    pub at: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EmployeeChain {
    pub schema_version: u32,
    pub id: Uuid,
    pub request_id: Uuid,
    /// Original normalized request retained for conflicting reuse detection after revisions.
    pub request: ChainDefinition,
    pub supervisor_id: Option<Uuid>,
    pub definition_version: u64,
    pub revision: u64,
    pub current_step: usize,
    pub status: ChainStatus,
    pub reason: Option<String>,
    pub finisher_reserved: bool,
    pub steps: Vec<ChainStep>,
    /// One durable, unread exception per incident; redelivery is read-only.
    pub attention: Option<ChainAttention>,
    pub history: Vec<ChainHistory>,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChainAttention {
    pub id: Uuid,
    pub reason: String,
    pub handled_at: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ChainOperation {
    Create {
        request_id: Uuid,
        definition: ChainDefinition,
    },
    Show {
        chain_id: Uuid,
    },
    Control {
        chain_id: Uuid,
        expected_revision: u64,
        control: ChainControl,
        #[serde(default)]
        source_revision: Option<String>,
    },
    Revise {
        chain_id: Uuid,
        expected_revision: u64,
        expected_definition_version: u64,
        step: usize,
        definition: ChainStepDefinition,
    },
    Approve {
        chain_id: Uuid,
        expected_revision: u64,
        gate: String,
        approved: bool,
    },
    RecordResult {
        chain_id: Uuid,
        attempt_id: Uuid,
        definition_version: u64,
        result: ChainResult,
    },
}
