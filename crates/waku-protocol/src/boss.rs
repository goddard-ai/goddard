//! Daemon-owned Boss identities, personas, files, and employee relationships.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{AgentSession, ProviderKind, RuntimeMode};

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossIdentity {
    pub id: Uuid,
    pub name: String,
    pub avatar_seed: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct PersonaPermissions {
    pub memory_folders: Vec<String>,
    pub integration_ids: Vec<String>,
    pub summon_employees: bool,
    pub computer_use: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossPersona {
    pub id: Uuid,
    pub name: String,
    pub markdown: String,
    /// Paths relative to the Boss files root.
    pub knowledge_files: Vec<String>,
    pub permissions: PersonaPermissions,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossEmployee {
    pub session_id: Uuid,
    pub supervisor_id: Uuid,
    pub identity: BossIdentity,
    #[serde(default)]
    pub job_title: String,
    pub persona_id: Uuid,
    /// The grants assigned when this employee was summoned.
    pub permissions: PersonaPermissions,
    #[serde(default)]
    pub knowledge_files: Vec<String>,
    pub expired: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossState {
    pub identity: BossIdentity,
    pub persona_id: Uuid,
    pub session_id: Option<Uuid>,
    pub personas: Vec<BossPersona>,
    pub employees: Vec<BossEmployee>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossFile {
    pub path: String,
    pub directory: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BossOperation {
    View,
    Open {
        project_id: Uuid,
        provider: ProviderKind,
        model: Option<String>,
        mode: RuntimeMode,
    },
    Summon {
        persona_id: Uuid,
        #[serde(alias = "name")]
        job_title: String,
        prompt: String,
        project: String,
        #[serde(default)]
        provider: Option<ProviderKind>,
        #[serde(default)]
        model: Option<String>,
    },
    Control {
        session_id: Uuid,
        action: EmployeeControl,
    },
    Transcript {
        session_id: Uuid,
        #[serde(default)]
        turn: Option<usize>,
    },
    Rename {
        name: String,
    },
    UpsertPersona {
        persona: BossPersona,
    },
    ListFiles {
        path: String,
    },
    ReadFile {
        path: String,
    },
    WriteFile {
        path: String,
        content: String,
    },
    CreateFolder {
        path: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BossResult {
    State {
        state: BossState,
    },
    Files {
        files: Vec<BossFile>,
    },
    File {
        path: String,
        content: String,
    },
    Saved,
    Session {
        session: Box<AgentSession>,
    },
    Summoned {
        session_id: Uuid,
    },
    Transcript {
        transcript: crate::model::AgentSessionTranscript,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EmployeeControl {
    Prompt { prompt: String },
    Steer { prompt: String },
    Stop,
}
