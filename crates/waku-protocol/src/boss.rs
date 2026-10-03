//! Daemon-owned Boss identities, personas, files, and employee relationships.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{AgentSession, Project, ProviderKind, RuntimeMode};
use crate::AgentWorkspace;

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
    /// Default icon shown for employees using this persona.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<crate::custom_commands::CustomCommandIcon>,
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
    /// Optional per-employee override of the persona icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<crate::custom_commands::CustomCommandIcon>,
    /// The grants assigned when this employee was summoned.
    pub permissions: PersonaPermissions,
    #[serde(default)]
    pub knowledge_files: Vec<String>,
    pub expired: bool,
}

/// A file or folder of employee output the boss published to the user's
/// sidebar. `path` is absolute on the daemon's host — employees run in their
/// assigned project directory, so bundles point outside the Boss files root.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossBundle {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    /// Recorded at publish time so renderers never stat the filesystem.
    pub directory: bool,
    pub created_at: u64,
    /// Re-publishing a path bumps this; the sidebar's recency window reads it.
    pub updated_at: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossState {
    pub identity: BossIdentity,
    pub persona_id: Uuid,
    pub session_id: Option<Uuid>,
    pub personas: Vec<BossPersona>,
    pub employees: Vec<BossEmployee>,
    #[serde(default)]
    pub bundles: Vec<BossBundle>,
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
    /// A bounded digest of the user's projects, tasks, and automations —
    /// the same snapshot the context router attaches to boss prompts.
    Context,
    Open {
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
        /// Where the employee's checkout runs; `None` uses the project
        /// itself, `worktree` forks a daemon-managed Git worktree.
        #[serde(default)]
        workspace: Option<AgentWorkspace>,
        /// The ref a worktree summon starts from; required when
        /// `workspace` is `worktree`, ignored otherwise.
        #[serde(default)]
        base_branch: Option<String>,
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
    SetEmployeeIcon {
        session_id: Uuid,
        #[serde(default)]
        icon: Option<crate::custom_commands::CustomCommandIcon>,
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
    /// Voice an utterance through every connected client's speech pipeline.
    /// `parts` are the spoken fragments in order: one element speaks a
    /// whole message, while several chain into a sentence — splitting on
    /// reusable boundaries (proper nouns, stock phrases) lets clients reuse
    /// previously synthesized clips instead of generating fresh audio.
    Speak {
        parts: Vec<String>,
    },
    /// Publish a file or folder to the user's sidebar. `path` is absolute on
    /// this daemon's host; `name` defaults to the path's file name.
    PublishBundle {
        path: String,
        #[serde(default)]
        name: Option<String>,
    },
    DismissBundle {
        id: Uuid,
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
    Context {
        context: String,
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
        project: Box<Project>,
    },
    Summoned {
        session_id: Uuid,
    },
    Transcript {
        transcript: crate::model::AgentSessionTranscript,
    },
    /// The `speak` request was broadcast to this many client connections.
    /// Each receiving client decides whether its voice settings voice it —
    /// zero means no client could possibly have heard it.
    Speak {
        delivered: usize,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EmployeeControl {
    Prompt { prompt: String },
    Steer { prompt: String },
    Stop,
}
