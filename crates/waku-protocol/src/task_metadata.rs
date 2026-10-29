//! User-owned task metadata and atomic edits, independent of transcript saves.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::model::AgentSession;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TaskMetadataField {
    Title,
    Placement,
    Archive,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TaskMetadataEdit {
    Rename {
        title: String,
    },
    Archive {
        archived_at: Option<u64>,
    },
    Placement {
        dormant_at: Option<u64>,
        dormant_exempt_until: Option<u64>,
        pinned_at: Option<u64>,
    },
}

impl TaskMetadataEdit {
    pub fn capture(field: TaskMetadataField, session: &AgentSession) -> Self {
        match field {
            TaskMetadataField::Title => Self::Rename {
                title: session.title.clone(),
            },
            TaskMetadataField::Archive => Self::Archive {
                archived_at: session.archived_at,
            },
            TaskMetadataField::Placement => Self::Placement {
                dormant_at: session.dormant_at,
                dormant_exempt_until: session.dormant_exempt_until,
                pinned_at: session.pinned_at,
            },
        }
    }

    pub fn field(&self) -> TaskMetadataField {
        match self {
            Self::Rename { .. } => TaskMetadataField::Title,
            Self::Archive { .. } => TaskMetadataField::Archive,
            Self::Placement { .. } => TaskMetadataField::Placement,
        }
    }

    pub fn apply(&self, session: &mut AgentSession) {
        match self {
            Self::Rename { title } => {
                session.title = title.trim().to_owned();
                session.agent_rename_allowed = false;
            }
            Self::Archive { archived_at } => session.archived_at = *archived_at,
            Self::Placement {
                dormant_at,
                dormant_exempt_until,
                pinned_at,
            } => {
                session.dormant_at = *dormant_at;
                session.dormant_exempt_until = *dormant_exempt_until;
                session.pinned_at = *pinned_at;
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct TaskMetadataUpdate {
    pub session_id: Uuid,
    pub edit: TaskMetadataEdit,
}

/// The fields an older transcript save must not write back over newer edits.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskMetadata {
    title: String,
    auto_title: Option<String>,
    agent_rename_allowed: bool,
    archived_at: Option<u64>,
    pinned_at: Option<u64>,
    dormant_at: Option<u64>,
    dormant_exempt_until: Option<u64>,
}

impl TaskMetadata {
    pub fn capture(session: &AgentSession) -> Self {
        Self {
            title: session.title.clone(),
            auto_title: session.auto_title.clone(),
            agent_rename_allowed: session.agent_rename_allowed,
            archived_at: session.archived_at,
            pinned_at: session.pinned_at,
            dormant_at: session.dormant_at,
            dormant_exempt_until: session.dormant_exempt_until,
        }
    }

    pub fn apply(self, session: &mut AgentSession) {
        session.title = self.title;
        session.auto_title = self.auto_title;
        session.agent_rename_allowed = self.agent_rename_allowed;
        session.archived_at = self.archived_at;
        session.pinned_at = self.pinned_at;
        session.dormant_at = self.dormant_at;
        session.dormant_exempt_until = self.dormant_exempt_until;
    }
}
