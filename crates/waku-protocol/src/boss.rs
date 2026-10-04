//! Daemon-owned Boss identities, personas, files, and employee relationships.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::AgentWorkspace;
use crate::model::{AgentSession, Project, ProviderKind, RuntimeMode};

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
    /// The boss's per-persona opt-in to prompted reports: employees of this
    /// persona deliver their finish report instead of expiring silently.
    pub always_report: bool,
}

impl PersonaPermissions {
    /// Restrict these grants to what `ceiling` holds — delegation can
    /// narrow, never widen, a supervisor employee's own permissions.
    pub fn clamp_within(&mut self, ceiling: &PersonaPermissions) {
        self.memory_folders.retain(|folder| {
            ceiling
                .memory_folders
                .iter()
                .any(|grant| std::path::Path::new(folder).starts_with(grant))
        });
        self.integration_ids
            .retain(|id| ceiling.integration_ids.contains(id));
        self.summon_employees &= ceiling.summon_employees;
        self.computer_use &= ceiling.computer_use;
        self.always_report &= ceiling.always_report;
    }
}

/// Per-field grant overrides attached to an individual employee — each
/// `Some` replaces that grant outright (an empty list clears it), while
/// `None` leaves the inherited or current value unchanged. Summon applies
/// them on top of the persona's permissions; the `setPermissions` control
/// action applies them to the employee's live record. Either way an
/// employee summoner stays clamped to its own grants.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct PermissionOverrides {
    pub memory_folders: Option<Vec<String>>,
    pub integration_ids: Option<Vec<String>>,
    pub summon_employees: Option<bool>,
    pub computer_use: Option<bool>,
}

impl PermissionOverrides {
    pub fn apply_to(&self, permissions: &mut PersonaPermissions) {
        if let Some(folders) = &self.memory_folders {
            permissions.memory_folders = folders.clone();
        }
        if let Some(ids) = &self.integration_ids {
            permissions.integration_ids = ids.clone();
        }
        if let Some(flag) = self.summon_employees {
            permissions.summon_employees = flag;
        }
        if let Some(flag) = self.computer_use {
            permissions.computer_use = flag;
        }
    }
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
    /// Unix timestamp when the employee finished. Retired after one hour
    /// unless the boss assigns the employee another prompt first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expired_at: Option<u64>,
    /// The attention item the employee raised through `reportBlocker`, or
    /// one the daemon recorded on its behalf (a restart interrupted the
    /// job). Its presence turns a finish back into a delivered report;
    /// resurrection clears it with the job that raised it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
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
    /// A pinned bundle keeps its sidebar row past the recency window.
    #[serde(default)]
    pub pinned_at: Option<u64>,
    /// A swept bundle hides behind the group's dormant fold until restored.
    #[serde(default)]
    pub dormant_at: Option<u64>,
    /// An archived bundle leaves the sidebar entirely.
    #[serde(default)]
    pub archived_at: Option<u64>,
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
        /// Per-field grant overrides persisted on the employee record;
        /// `None` inherits the persona's permissions unchanged.
        #[serde(default)]
        permissions: Option<PermissionOverrides>,
    },
    Control {
        session_id: Uuid,
        action: EmployeeControl,
    },
    /// An employee flags that its job needs supervisor attention — a
    /// blocker, a decision, or a failure. The report interrupts the
    /// supervisor's running turn when the runtime can take it and stays on
    /// the record so the finish delivers a full report instead of expiring
    /// silently. Employee-only.
    ReportBlocker {
        message: String,
    },
    Transcript {
        session_id: Uuid,
        #[serde(default)]
        turn: Option<usize>,
    },
    Rename {
        name: String,
    },
    /// Rename an employee's identity. The boss itself uses `Rename`.
    RenameEmployee {
        session_id: Uuid,
        name: String,
    },
    /// Re-roll a managed identity's avatar seed so it draws a new face.
    /// `None` — or the boss's own session id — targets the boss.
    RegenerateAvatar {
        session_id: Option<Uuid>,
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
    /// Pin or unpin a bundle's sidebar row; pinned bundles lead the group
    /// and never age out of it.
    PinBundle {
        id: Uuid,
        pinned: bool,
    },
    /// Sweep a bundle behind the group's dormant fold, or restore it to the
    /// live list.
    SweepBundle {
        id: Uuid,
        dormant: bool,
    },
    /// Archive or unarchive a bundle — archived bundles leave the sidebar
    /// but keep their record.
    ArchiveBundle {
        id: Uuid,
        archived: bool,
    },
    /// Run a Rhai script inside the daemon with the other boss operations
    /// bound as native functions — one call batches operations and chains
    /// their results. Variables persist in the boss session's eval scope
    /// between calls. Boss-only.
    Eval {
        script: String,
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
    /// A finished `eval` script's return value plus the text its
    /// `print`/`debug` calls emitted (bounded). `value` is `null` when the
    /// script returns `()`.
    Eval {
        value: serde_json::Value,
        output: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum EmployeeControl {
    Prompt { prompt: String },
    Steer { prompt: String },
    /// Apply a catalog-listed provider/model selection to the next turn.
    SetModel {
        provider: crate::model::ProviderKind,
        model: String,
        #[serde(default)]
        reasoning_effort: Option<String>,
    },
    /// Replace individual grants on the employee's record — memory and
    /// delegation changes take effect immediately, while MCP server and
    /// Computer Use grants apply to the employee's next launch. An
    /// employee supervisor cannot raise a grant past its own.
    SetPermissions {
        permissions: PermissionOverrides,
    },
    Stop,
}

#[cfg(test)]
mod tests {
    use super::EmployeeControl;

    #[test]
    fn set_model_control_decodes_provider_model_and_effort() {
        let action: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setModel",
            "provider": "codex",
            "model": "gpt-5.5",
            "reasoningEffort": "high"
        }))
        .unwrap();
        assert!(matches!(
            action,
            EmployeeControl::SetModel { provider, model, reasoning_effort }
                if provider == crate::model::ProviderKind::Codex
                    && model == "gpt-5.5"
                    && reasoning_effort.as_deref() == Some("high")
        ));
    }

    #[test]
    fn summon_and_set_permissions_decode_grant_overrides() {
        let summon: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "summon",
            "personaId": "00000000-0000-0000-0000-000000000001",
            "jobTitle": "Verifier",
            "prompt": "Check the build",
            "project": "/project",
            "permissions": {"memoryFolders": ["work"], "computerUse": true}
        }))
        .unwrap();
        assert!(matches!(
            summon,
            super::BossOperation::Summon { permissions: Some(overrides), .. }
                if overrides.memory_folders.as_deref() == Some(&["work".to_string()][..])
                    && overrides.computer_use == Some(true)
                    && overrides.integration_ids.is_none()
        ));
        let action: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setPermissions",
            "permissions": {"integrationIds": []}
        }))
        .unwrap();
        assert!(matches!(
            action,
            EmployeeControl::SetPermissions { permissions }
                if permissions.integration_ids == Some(Vec::new())
                    && permissions.summon_employees.is_none()
        ));
    }
}
