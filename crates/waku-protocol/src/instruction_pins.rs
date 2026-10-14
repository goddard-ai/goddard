//! Temporary, source-backed user obligations owned by the Boss daemon.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InstructionPinState {
    Candidate,
    Active,
    Retired,
    Superseded,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstructionPinSource {
    pub session_id: Uuid,
    pub message_id: Uuid,
    /// Original user text, retained even when the source chat is cleared.
    pub quote: String,
    #[serde(default)]
    pub quote_truncated: bool,
}

/// Only an explicit linked condition permits automatic retirement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum InstructionPinRetirement {
    OutcomeCompleted { outcome_id: Uuid },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstructionPinResolution {
    pub at: u64,
    pub evidence: String,
    pub superseded_by: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstructionPin {
    pub id: Uuid,
    pub revision: u64,
    pub instruction: String,
    pub retire_when: String,
    pub project_id: Option<Uuid>,
    pub retire_on: Option<InstructionPinRetirement>,
    pub source: Option<InstructionPinSource>,
    pub state: InstructionPinState,
    pub created_at: u64,
    pub updated_at: u64,
    pub resolution: Option<InstructionPinResolution>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum InstructionPinOperation {
    Show {
        id: Uuid,
    },
    List {
        #[serde(default)]
        include_retired: bool,
    },
    Pin {
        instruction: String,
        retire_when: String,
        #[serde(default)]
        project_id: Option<Uuid>,
        #[serde(default)]
        retire_on: Option<InstructionPinRetirement>,
        #[serde(default)]
        source_session_id: Option<Uuid>,
        #[serde(default)]
        source_message_id: Option<Uuid>,
    },
    /// Editing a candidate activates it. Revision matching prevents a stale
    /// client from restoring an instruction another client retired or edited.
    Edit {
        id: Uuid,
        expected_revision: u64,
        instruction: String,
        retire_when: String,
        #[serde(default)]
        project_id: Option<Uuid>,
        #[serde(default)]
        retire_on: Option<InstructionPinRetirement>,
    },
    Retire {
        id: Uuid,
        expected_revision: u64,
        evidence: String,
        #[serde(default)]
        superseded_by: Option<Uuid>,
    },
}
