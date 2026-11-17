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
    /// Settled human-answer turns that already claimed a recommendation
    /// pass — the durable dedupe identity for the automatic follow-up.
    #[serde(default)]
    pub recommendation_turns: Vec<Uuid>,
    /// Counsel's managed identity, seeded lazily by the first Open that
    /// finds it missing so documents predating the field upgrade in place.
    #[serde(default)]
    pub identity: Option<crate::boss::BossIdentity>,
    #[serde(default)]
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CounselStatus {
    Proposed,
    Later,
    Dismissed,
    /// The human approved this exact revision; Counsel has been asked to
    /// deliver it but has not invoked the send operation yet.
    Approved,
    /// Counsel invoked send and the instruction was committed to Boss's
    /// delivery pipeline — durable mirror parked, steer or drain in flight.
    /// Persistence alone never produces this state; only a delivery attempt
    /// moves a record here.
    DeliveryPending,
    /// The last delivery attempt failed. The record carries
    /// `delivery_error`; a retry is a fresh attempt, not a duplicate.
    DeliveryFailed,
    /// The instruction reached Boss's live surface — a steer into the open
    /// turn or a committed queue drain. Sent is not completion.
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
    /// Counsel chat message the card renders after — its originating
    /// position in history. `None` on records written before anchoring;
    /// clients render those at the transcript's end.
    #[serde(default)]
    pub anchor: Option<Uuid>,
    /// Why the last send attempt failed; cleared by each new attempt.
    #[serde(default)]
    pub delivery_error: Option<String>,
    /// The revision the human approved — the binding `Send` revalidates.
    /// Bookkeeping bumps after approval must not invalidate it.
    #[serde(default)]
    pub approved_revision: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CounselDecision {
    /// Explicit human approval of the displayed revision and its exact
    /// proposed message. A conversational yes is never this decision —
    /// approval resumes Counsel so it can deliver through `Send`.
    Approve,
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
    /// Counsel's one authorized write to Boss, legal only on a record the
    /// human already approved at this revision. The instruction delivered
    /// is the record's stored message — Counsel cannot supply a different
    /// one. Idempotent across retries, redeliveries, and rotation.
    Send { id: Uuid, expected_revision: u64 },
    /// Human rename of the persistent Counsel identity.
    Rename { name: String },
    /// Human re-roll of Counsel's avatar seed.
    RegenerateAvatar,
    Complete {
        id: Uuid,
        expected_revision: u64,
        evidence: String,
    },
}
