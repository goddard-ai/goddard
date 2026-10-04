//! Daemon-owned Boss identities, personas, files, and employee relationships.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::AgentWorkspace;
use crate::automations::AutomationInput;
use crate::model::{AgentSession, Project, ProviderKind, RuntimeMode, SessionPlanning};

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
    /// Memory files pinned into the persona's context — paths relative to
    /// `memory/` in the Boss files root, matching `memory_folders`.
    /// Pinning also grants employees read access to the file.
    #[serde(default, alias = "knowledgeFiles")]
    pub pinned_files: Vec<String>,
    pub permissions: PersonaPermissions,
    /// Default icon shown for employees using this persona.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<crate::custom_commands::CustomCommandIcon>,
}

/// The kind of bounded work a summon fixes. The boss picks it in the
/// summon payload and it never changes afterward — it decides what a
/// finish does, not the employee's persona or live state.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EmployeeGoal {
    /// The supervisor needs the completion — it chains into the next job
    /// or the human's attention — so the finish delivers the employee's
    /// transcript index as a report.
    #[default]
    Errand,
    /// Fire-and-forget work: the finish prompts nobody. The record stays
    /// on the daemon's roster past the one-hour retirement window so the
    /// client's Goals page can list it.
    Goal,
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
    /// The work kind fixed at summon; records written before it existed
    /// deserialize as `Errand`.
    #[serde(default)]
    pub work_goal: EmployeeGoal,
    /// Unix timestamp of the summon — the Goals page's started time.
    /// `None` on records older than the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    /// Optional per-employee override of the persona icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<crate::custom_commands::CustomCommandIcon>,
    /// The grants assigned when this employee was summoned.
    pub permissions: PersonaPermissions,
    /// The persona's memory pins at summon time — paths relative to
    /// `memory/` in the Boss files root.
    #[serde(default, alias = "knowledgeFiles")]
    pub pinned_files: Vec<String>,
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
    /// When the user last opened the bundle. `None` — or older than
    /// `updated_at` after a re-publish — reads as unread in the sidebar.
    #[serde(default)]
    pub viewed_at: Option<u64>,
}

/// A planning session the boss opened with `createPlan`: the managed task
/// it drafts in and the plan document that task owns. `plan_file` is
/// relative to `memory/` in the Boss files root — always `plans/<name>.md`.
/// `finalized_at` freezes the document once the user approves
/// `finalizePlan`; the daemon archives the session after a grace period.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossPlan {
    pub session_id: Uuid,
    pub plan_file: String,
    /// What the session is planning — the session's title carries the same
    /// text so the sidebar row reads as the idea.
    pub idea: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<u64>,
}

impl BossPlan {
    /// The session-facing half of the record — everything the task list
    /// needs to badge and order a planning row without the Boss document.
    pub fn session_planning(&self) -> SessionPlanning {
        SessionPlanning {
            plan_file: self.plan_file.clone(),
            idea: self.idea.clone(),
            label: crate::protocol::WireTranslation::new("boss.planning_label", []),
            finalized_at: self.finalized_at,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossState {
    pub identity: BossIdentity,
    pub persona_id: Uuid,
    pub session_id: Option<Uuid>,
    pub personas: Vec<BossPersona>,
    pub employees: Vec<BossEmployee>,
    /// Retired identities remain available for revival until their name is
    /// assigned to another employee. They are not part of the visible roster.
    #[serde(default)]
    pub retired_employees: Vec<BossEmployee>,
    #[serde(default)]
    pub bundles: Vec<BossBundle>,
    /// Open planning sessions and their plan documents. Records stay after
    /// finalization and archive — the freeze they carry is permanent.
    #[serde(default)]
    pub planning: Vec<BossPlan>,
    /// The daemon clock when the user last had the Goals page open —
    /// a goal finished since then reads as unread in the sidebar, the
    /// same contract `BossBundle::viewed_at` gives its row.
    #[serde(default)]
    pub goals_viewed_at: Option<u64>,
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
    /// A compact text summary of employee state, without session transcripts.
    Roster,
    /// A bounded digest of the user's projects, tasks, and automations —
    /// the same snapshot the context router attaches to boss prompts.
    Context,
    Open {
        provider: ProviderKind,
        model: Option<String>,
        mode: RuntimeMode,
    },
    /// Open a dedicated planning session on an idea. The session is a
    /// managed boss principal — the boss identity agents it — seeded with
    /// `prompt` (the user request that prompted planning) plus a canned
    /// opener. `plan_file` names the plan document under `plans/` in the
    /// Boss memory root; `title` is the idea the sidebar row displays.
    /// Boss-only; the user never creates one directly.
    CreatePlan {
        title: String,
        plan_file: String,
        prompt: String,
        #[serde(default)]
        provider: Option<ProviderKind>,
        #[serde(default)]
        model: Option<String>,
    },
    /// Open a web page in the boss chat's right panel. Boss principals only.
    Browse {
        url: String,
        #[serde(default)]
        title: Option<String>,
    },
    /// Freeze a plan document after user approval. A planning session
    /// finalizes its own plan (`plan_file` omitted); the boss chat or a
    /// human names the file. Approval lands on a daemon-owned request card;
    /// the session archives once the grace period elapses.
    FinalizePlan {
        #[serde(default)]
        plan_file: Option<String>,
    },
    /// Manage the user's daemon-owned scheduled automations. Boss-only.
    Automation {
        action: AutomationOperation,
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
        /// The work kind fixed at summon — `errand` (the default) reports
        /// its finish to the supervisor; `goal` expires silently and lists
        /// on the client's Goals page.
        #[serde(default)]
        work_goal: EmployeeGoal,
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
    /// Direct deterministic access to the Boss's file-canonical memory store.
    Memory {
        operation: MemoryOperation,
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
    /// Stamp the bundle viewed at the daemon's clock — opening its page or
    /// preview retires the sidebar's unread marker.
    MarkBundleViewed {
        id: Uuid,
    },
    /// Stamp the Goals page viewed at the daemon's clock — goal finishes
    /// older than the stamp stop counting toward the sidebar's unread dot.
    MarkGoalsViewed,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AutomationOperation {
    List,
    Create { input: AutomationInput },
    Update { input: AutomationInput },
    Delete { automation_id: Uuid },
    Pause { automation_id: Uuid },
    Resume { automation_id: Uuid },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MemoryOperation {
    Insert {
        collection: String,
        title: String,
        cue: String,
        body: String,
        source_id: String,
    },
    ImportFolder {
        folder: String,
        collection: String,
    },
    Surface {
        collection: String,
        limit: usize,
    },
    ListIndex,
    Search {
        collection: String,
        query: String,
    },
    ReadChunk {
        collection: String,
        chunk_id: String,
    },
    Zoom {
        collection: String,
        target: String,
    },
}

/// One immutable memory record. The daemon's file store persists it as a
/// Markdown file: snake_case frontmatter (the field aliases) plus `body` as
/// the file's text, while the wire form is camelCase like every other
/// protocol type.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryChunk {
    pub version: u32,
    #[serde(alias = "chunk_id")]
    pub chunk_id: String,
    #[serde(alias = "scope_id")]
    pub scope_id: String,
    #[serde(alias = "collection_id")]
    pub collection_id: String,
    pub layer: String,
    pub revision: u64,
    pub title: String,
    pub cue: String,
    pub status: String,
    #[serde(alias = "source_id")]
    pub source_id: String,
    #[serde(alias = "source_digest")]
    pub source_digest: String,
    #[serde(alias = "created_at")]
    pub created_at: u64,
    #[serde(default)]
    pub body: String,
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
    Roster {
        roster: String,
    },
    Context {
        context: String,
    },
    Automations {
        state: crate::automations::AutomationsState,
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
    /// The client should reveal the URL in the given managed boss session.
    Browse {
        session_id: Uuid,
        url: String,
        title: Option<String>,
    },
    /// A plan document froze on user approval: its path beneath `memory/`
    /// in the Boss files root and the freeze stamp. The owning session
    /// archives once the grace period elapses.
    PlanFinalized {
        session_id: Uuid,
        plan_file: String,
        finalized_at: u64,
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
    Memory {
        index: Option<String>,
        chunks: Vec<MemoryChunk>,
        inserted: Option<MemoryChunk>,
        imported: Option<usize>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EmployeeControl {
    Prompt {
        prompt: String,
    },
    Steer {
        prompt: String,
    },
    /// Apply a catalog-listed provider/model selection to the next turn.
    SetModel {
        provider: crate::model::ProviderKind,
        model: String,
        #[serde(default)]
        reasoning_effort: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        interrupt: Option<bool>,
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
            "reasoningEffort": "high",
            "interrupt": true
        }))
        .unwrap();
        assert!(matches!(
            action,
            EmployeeControl::SetModel { provider, model, reasoning_effort, interrupt }
                if provider == crate::model::ProviderKind::Codex
                    && model == "gpt-5.5"
                    && reasoning_effort.as_deref() == Some("high")
                    && interrupt == Some(true)
        ));
    }

    #[test]
    fn set_model_control_defaults_interrupt_to_false() {
        let action: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setModel",
            "provider": "codex",
            "model": "gpt-5.5"
        }))
        .unwrap();
        assert!(matches!(action, EmployeeControl::SetModel { interrupt: None, .. }));
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
            super::BossOperation::Summon { permissions: Some(overrides), work_goal, .. }
                if overrides.memory_folders.as_deref() == Some(&["work".to_string()][..])
                    && overrides.computer_use == Some(true)
                    && overrides.integration_ids.is_none()
                    // An omitted kind is an errand: its finish reports.
                    && work_goal == super::EmployeeGoal::Errand
        ));
        let goal: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "summon",
            "personaId": "00000000-0000-0000-0000-000000000001",
            "jobTitle": "Watcher",
            "prompt": "Watch the queue",
            "project": "/project",
            "workGoal": "goal"
        }))
        .unwrap();
        assert!(matches!(
            goal,
            super::BossOperation::Summon { work_goal: super::EmployeeGoal::Goal, .. }
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

    #[test]
    fn plan_operations_and_results_decode_camel_case_wire_payloads() {
        let create: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "createPlan",
            "title": "Auth migration",
            "planFile": "auth.md",
            "prompt": "Plan the auth migration"
        }))
        .unwrap();
        assert!(matches!(
            create,
            super::BossOperation::CreatePlan { title, plan_file, prompt, provider, model }
                if title == "Auth migration" && plan_file == "auth.md"
                    && prompt == "Plan the auth migration"
                    && provider.is_none() && model.is_none()
        ));
        // A planning session finalizes its own plan by omitting the file.
        let finalize: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "finalizePlan"
        }))
        .unwrap();
        assert!(matches!(
            finalize,
            super::BossOperation::FinalizePlan { plan_file: None }
        ));
        let result: super::BossResult = serde_json::from_value(serde_json::json!({
            "type": "planFinalized",
            "sessionId": "00000000-0000-0000-0000-000000000001",
            "planFile": "plans/auth.md",
            "finalizedAt": 1_700_000_000
        }))
        .unwrap();
        assert!(matches!(
            result,
            super::BossResult::PlanFinalized { plan_file, finalized_at, .. }
                if plan_file == "plans/auth.md" && finalized_at == 1_700_000_000
        ));
    }

    #[test]
    fn browse_operation_and_result_decode_camel_case_wire_payloads() {
        let operation: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "browse",
            "url": "https://example.com/docs",
            "title": "Documentation"
        }))
        .unwrap();
        assert!(matches!(
            operation,
            super::BossOperation::Browse { url, title: Some(title) }
                if url == "https://example.com/docs" && title == "Documentation"
        ));
        let result: super::BossResult = serde_json::from_value(serde_json::json!({
            "type": "browse",
            "sessionId": "00000000-0000-0000-0000-000000000001",
            "url": "https://example.com/docs",
            "title": null
        }))
        .unwrap();
        assert!(matches!(result, super::BossResult::Browse { url, title: None, .. } if url == "https://example.com/docs"));
    }

    #[test]
    fn a_boss_state_saved_before_planning_decodes_with_an_empty_registry() {
        let state: super::BossState = serde_json::from_value(serde_json::json!({
            "identity": {
                "id": "00000000-0000-0000-0000-000000000001",
                "name": "Boss",
                "avatarSeed": "seed"
            },
            "personaId": "00000000-0000-0000-0000-000000000002",
            "sessionId": null,
            "personas": [],
            "employees": [],
            "bundles": [],
            "revision": 0
        }))
        .unwrap();
        assert!(state.planning.is_empty());
    }
}
