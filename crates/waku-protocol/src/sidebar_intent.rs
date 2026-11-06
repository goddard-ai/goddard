//! Durable, ordered client edits to sidebar organization.
use crate::{boss::BossState, model::AgentSession, task_metadata::TaskMetadataEdit};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SidebarEdit {
    Task { id: Uuid, edit: TaskMetadataEdit },
    DeliverableArchive { id: Uuid, archived: bool },
    DeliverablePin { id: Uuid, pinned: bool },
    DeliverableDormant { id: Uuid, dormant: bool },
}
impl SidebarEdit {
    pub fn key(&self) -> String {
        match self {
            Self::Task { id, edit } => {
                let field = match edit.field() {
                    crate::task_metadata::TaskMetadataField::Title => "title",
                    crate::task_metadata::TaskMetadataField::Placement => "placement",
                    crate::task_metadata::TaskMetadataField::Archive => "archive",
                };
                format!("task:{id}:{field}")
            }
            Self::DeliverableArchive { id, .. } => format!("deliverable:{id}:archive"),
            Self::DeliverablePin { id, .. } => format!("deliverable:{id}:pin"),
            Self::DeliverableDormant { id, .. } => format!("deliverable:{id}:dormant"),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, TS)]
pub struct SidebarIntent {
    pub client_id: Uuid,
    pub generation: u64,
    pub operation_id: Uuid,
    pub expected_revision: Option<u64>,
    pub expected: Option<SidebarEdit>,
    pub edit: SidebarEdit,
}
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SidebarIntentResult {
    Task { session: Box<AgentSession> },
    Deliverables { state: Box<BossState> },
    Blocked { reason: String },
    Superseded,
    Removed,
}
