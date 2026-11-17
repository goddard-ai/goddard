//! The human's advisory chat and its explicit handoffs to Boss.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CounselState {
    pub session_id: Option<Uuid>,
    #[serde(default)]
    pub recommendations: Vec<CounselRecommendation>,
    #[serde(default)]
    pub reviewed_activity: Option<String>,
    #[serde(default)]
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CounselStatus {
    Proposed,
    Later,
    Dismissed,
    Sent,
    Completed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CounselSource {
    pub session_id: Uuid,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CounselRecommendation {
    pub id: Uuid,
    pub revision: u64,
    pub title: String,
    pub reason: String,
    /// Exact instruction the human reviews before sending.
    pub instruction: String,
    pub sources: Vec<CounselSource>,
    pub status: CounselStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub boss_session_id: Option<Uuid>,
    pub completion_evidence: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CounselDecision {
    Send,
    Later,
    Dismiss,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CounselOperation {
    /// Human navigation; creates or returns the persistent chat.
    Open {
        provider: crate::model::ProviderKind,
        model: Option<String>,
        mode: crate::model::RuntimeMode,
    },
    View,
    /// Counsel supplies evidence and drafts an instruction; this never sends it.
    Propose {
        title: String,
        reason: String,
        instruction: String,
        sources: Vec<Uuid>,
        #[serde(default)]
        replaces: Option<Uuid>,
    },
    /// Only a human client may decide. Revision matching binds approval to
    /// exactly the instruction visible when the control was activated.
    Decide {
        id: Uuid,
        expected_revision: u64,
        decision: CounselDecision,
    },
    Complete {
        id: Uuid,
        expected_revision: u64,
        evidence: String,
    },
}
