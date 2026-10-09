//! Daemon-owned Boss identities, personas, files, and employee relationships.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::AgentWorkspace;
use crate::automations::AutomationInput;
use crate::model::{AgentSession, Project, ProviderKind, RuntimeMode, SessionPlanning};

/// Human-selected avatar generator. `blobby` was removed and remaps to the
/// `gaze` default; identities written before the field existed deserialize
/// through [`legacy_avatar_style`] so their faces stay on DiceBear moods.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, Hash, TS)]
#[serde(rename_all = "camelCase")]
pub enum AvatarStyle {
    /// DiceBear "moods" — labeled Moods in the picker.
    DiceBear,
    /// DiceBear "gaze" — the app's default for new identities.
    #[default]
    #[serde(alias = "blobby")]
    Gaze,
    Dylan,
    FunEmoji,
    LineFace,
    AgentAvatars,
    Avvvatars,
}

/// `#[serde(default)]` for stored `avatarStyle` fields: records written
/// before the picker existed rendered DiceBear moods, so a missing value
/// stays on `DiceBear` even though the app's default is now `Gaze`.
pub(crate) fn legacy_avatar_style() -> AvatarStyle {
    AvatarStyle::DiceBear
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossIdentity {
    pub id: Uuid,
    pub name: String,
    pub avatar_seed: String,
    #[serde(default = "legacy_avatar_style")]
    pub avatar_style: AvatarStyle,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct PersonaPermissions {
    /// Explicit grants to Boss-created memory buckets.
    #[serde(default)]
    pub bucket_ids: Vec<String>,
    pub integration_ids: Vec<String>,
    pub summon_employees: bool,
    pub computer_use: bool,
}

impl PersonaPermissions {
    /// Restrict these grants to what `ceiling` holds — delegation can
    /// narrow, never widen, a supervisor employee's own permissions.
    pub fn clamp_within(&mut self, ceiling: &PersonaPermissions) {
        self.bucket_ids
            .retain(|bucket| ceiling.bucket_ids.contains(bucket));
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
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionOverrides {
    #[serde(default)]
    pub bucket_ids: Option<Vec<String>>,
    pub integration_ids: Option<Vec<String>>,
    pub summon_employees: Option<bool>,
    pub computer_use: Option<bool>,
}

impl PermissionOverrides {
    pub fn apply_to(&self, permissions: &mut PersonaPermissions) {
        if let Some(buckets) = &self.bucket_ids {
            permissions.bucket_ids = buckets.clone();
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
    /// Documents pinned into the persona's context — relative to the Boss
    /// files root. Memory knowledge is granted through named buckets.
    #[serde(default, alias = "knowledgeFiles")]
    pub pinned_files: Vec<String>,
    pub permissions: PersonaPermissions,
    /// Default icon shown for employees using this persona.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<crate::custom_commands::CustomCommandIcon>,
}

/// The upsert payload for a persona — identical to [`BossPersona`] except
/// `icon` is tri-state: an absent field preserves the stored default,
/// `null` clears it, and an identifier replaces it. A summon copies the
/// resolved icon onto the employee record, so later edits here never
/// reach existing employees.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossPersonaUpsert {
    /// Nil assigns a fresh id on write.
    pub id: Uuid,
    pub name: String,
    pub markdown: String,
    /// Documents pinned into the persona's context — relative to the Boss
    /// files root. Memory knowledge is granted through named buckets.
    #[serde(default, alias = "knowledgeFiles")]
    pub pinned_files: Vec<String>,
    pub permissions: PersonaPermissions,
    /// Employee icon default — absent preserves the stored value, `null`
    /// clears it, an identifier replaces it.
    #[serde(
        default,
        deserialize_with = "double_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub icon: Option<Option<crate::custom_commands::CustomCommandIcon>>,
}

/// Serde adapter for fields that must tell `null` apart from absent:
/// deserialization sees the raw value — `null` becomes `Some(None)` and a
/// value `Some(Some(..))` — while `#[serde(default)]` still covers the
/// absent case with `None`.
mod double_option {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        Option::<T>::deserialize(deserializer).map(Some)
    }
}

impl From<BossPersona> for BossPersonaUpsert {
    /// Round-trips a fetched record — `Some` makes the icon field
    /// always-replace, writing back exactly what the record held.
    fn from(persona: BossPersona) -> Self {
        Self {
            id: persona.id,
            name: persona.name,
            markdown: persona.markdown,
            pinned_files: persona.pinned_files,
            permissions: persona.permissions,
            icon: Some(persona.icon),
        }
    }
}

// ── Shipped persona defaults ────────────────────────────────────────────
//
// The canonical Boss and Employee personas ship instruction text that
// evolves with the app. The revision catalog below keeps every text a
// release has installed so a saved persona that byte-matches one is
// provably untouched — only those adopt a newer shipped text
// automatically, while customized or unknown-origin text stays put.
// Revision labels are per role and monotonic; when editing a shipped
// default, archive its text under the previous revision and bump the
// current entry.

/// The canonical instruction roles Goddard ships: `Boss` is the boss
/// chat's own persona, `Employee` the shared base every employee
/// composes, and the rest specialist employee roles an assignment can
/// select like a custom persona. Identity is stable across renames —
/// `BossState::persona_id`, [`BossState::employee_persona_id`], and
/// [`BossState::specialist_persona_ids`] name the records, never the
/// name, so a custom persona named "Employee" is never a default.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PersonaDefaultRole {
    Boss,
    Employee,
    Researcher,
    FeatureDeveloper,
    BugInvestigator,
    Verifier,
}

impl PersonaDefaultRole {
    /// Every canonical role in stable order — the base roles first,
    /// then the shipped specialists.
    pub const ALL: &'static [PersonaDefaultRole] = &[
        PersonaDefaultRole::Boss,
        PersonaDefaultRole::Employee,
        PersonaDefaultRole::Researcher,
        PersonaDefaultRole::FeatureDeveloper,
        PersonaDefaultRole::BugInvestigator,
        PersonaDefaultRole::Verifier,
    ];

    /// The shipped specialist employee roles — canonical records seeded
    /// for every install, layered on the Employee base when assigned.
    pub const SPECIALISTS: &'static [PersonaDefaultRole] = &[
        PersonaDefaultRole::Researcher,
        PersonaDefaultRole::FeatureDeveloper,
        PersonaDefaultRole::BugInvestigator,
        PersonaDefaultRole::Verifier,
    ];

    /// The role's display name — also the seeded record's default name.
    pub fn label(self) -> &'static str {
        match self {
            PersonaDefaultRole::Boss => "Boss",
            PersonaDefaultRole::Employee => "Employee",
            PersonaDefaultRole::Researcher => "Researcher",
            PersonaDefaultRole::FeatureDeveloper => "Feature Developer",
            PersonaDefaultRole::BugInvestigator => "Bug Investigator",
            PersonaDefaultRole::Verifier => "Verifier",
        }
    }

    /// The icon a freshly seeded record carries. Base roles seed without
    /// one — the Boss row is drawn separately and the Employee base keeps
    /// the generic persona glyph.
    pub fn default_icon(self) -> Option<crate::custom_commands::CustomCommandIcon> {
        use crate::custom_commands::CustomCommandIcon;
        match self {
            PersonaDefaultRole::Researcher => Some(CustomCommandIcon::Search),
            PersonaDefaultRole::FeatureDeveloper => Some(CustomCommandIcon::Compose),
            PersonaDefaultRole::BugInvestigator => Some(CustomCommandIcon::Bug),
            PersonaDefaultRole::Verifier => Some(CustomCommandIcon::CircleCheck),
            PersonaDefaultRole::Boss | PersonaDefaultRole::Employee => None,
        }
    }
}

/// One shipped revision of a canonical default's instructions —
/// `markdown` is the exact text that revision installed.
pub struct PersonaDefaultRevision {
    pub revision: u32,
    pub markdown: &'static str,
}

const EMPLOYEE_DEFAULT_V1: &str = "Complete the bounded job assigned by your supervisor. Report useful results concisely. You have no memory of your own and must not write memory. Read only the memory granted to or pinned by your persona.";

const EMPLOYEE_DEFAULT_V2: &str = "Complete the bounded job assigned by your supervisor. Report useful results concisely to your supervisor through your final response and full transcript. Publish a user-facing deliverable only when the human explicitly or implicitly asked for a report, document, or artifact; routine technical investigations, audits, matrices, and JSON evidence remain supervisor reports. Write requested Markdown documents in plain language with a descriptive title, purpose, context, useful headings, and recommendations. You have no memory of your own and must not write memory. Read only the memory granted to or pinned by your persona.";

/// Revision 3: the refined shared base — applies to every employee,
/// beneath any custom role, and grants memory writes the older texts
/// forbade.
const EMPLOYEE_DEFAULT_V3: &str = "Complete the bounded assignment from your supervisor in the assigned project or workspace. Follow the project's instructions and the additional custom persona, when present. The custom persona supplies specialist methods and limits; these shared Employee responsibilities remain applicable.\n\nWork independently within the authorized scope. Preserve unrelated work, credentials, and private data. Do not expand the assignment or delegate further unless authorized. If an ambiguity can be resolved safely from context, proceed; report a blocker when supervisor or human action is needed.\n\nUse memory actively within the access described by the shared runtime prompt. Retrieve relevant context for the assignment and record useful durable findings, decisions, and verified outcomes in the appropriate granted buckets. Distinguish facts from inference and temporary state. Keep role guidance in personas and contextual knowledge in memory. If a useful finding belongs in a bucket you cannot access, report it to your supervisor rather than placing it in an unrelated bucket. Use only granted knowledge, integrations, and capabilities.\n\nFor implementation work, follow local conventions, make focused changes, and run verification appropriate to the changed behavior. Use the project's shared build cache and resource-reservation rules where applicable. Finish the required commit and integration workflow; preserve work and report the commit and failure if landing cannot complete safely.\n\nHand off evidence appropriate to the assignment: committed and integrated changes for implementation, supported findings for research, and actual results for verification. Do not substitute patch snapshots for committed work.\n\nKeep project documentation in the project according to its conventions. Publish requested Boss-facing documents as deliverables. Keep routine investigation evidence in your report unless an artifact was requested.\n\nReport outcomes, verification, remaining limitations, and blockers clearly. Relate the result to the assigned outcome and work item; identify any unmet completion condition and remaining dependency. Completing your assignment does not imply the overall outcome is complete. Distinguish observed facts from assumptions. Use the designated completion flow, and interrupt your supervisor only when new information requires action during the work. Do not announce success before required work is complete.";

const EMPLOYEE_PERSONA_REVISIONS: &[PersonaDefaultRevision] = &[
    PersonaDefaultRevision {
        revision: 1,
        markdown: EMPLOYEE_DEFAULT_V1,
    },
    PersonaDefaultRevision {
        revision: 2,
        markdown: EMPLOYEE_DEFAULT_V2,
    },
    PersonaDefaultRevision {
        revision: 3,
        markdown: EMPLOYEE_DEFAULT_V3,
    },
];

const BOSS_DEFAULT_V1: &str = "You coordinate employees for the human. Heavy delegation is your default: assign code changes, research, internet access, builds, code generation, long-running checks and tests, for daemon-managed worktrees, require each assigned employee to include 'agent-merge per rules', commit its unit, and run 'goddard-agent merge submit' itself; do not summon a separate Worktree Integrator. Employees report conflicts or verification failures with 'reportBlocker' only when supervisor or human action is required and report the landed SHA on success. Never run or poll long-running commands yourself. Ask employees to use shared build caches or dedicated output directories when that avoids contention with the user's tools. Never poll, watch, or wait yourself — hand recurring checks and waits to an employee; mark each summon `workGoal` — an `errand` when you need to know when it finishes (its finish reports back to you), a `goal` when it finishes without you (its record lands on the human's Goals page instead) — a finish also reaches you when the employee flagged a blocker or its session failed. Use `roster` for a cheap status check, `view` for employee details, and `context` for the user's work. Verify completion from the worktree and its commits before reporting work done; do not rely on a summary alone.\n\nUse `steer` for mid-flight corrections that change what the employee is writing right now — a steer that redirects the assignment itself may carry `jobTitle` to relabel the job. Use `prompt` for content whose relevance starts after the current step, such as queue additions or follow-ups. Prompt or steer can resume an employee after its idle expiry with the same transcript; summon a fresh employee for a new or distinct job, or when the previous employee is dead or finishing; never stack prompts onto an expiring employee, where queued work may be lost.\n\nTrack employee ownership, worktrees, and landed versus in-flight work in the appropriate memory bucket, and reconcile the notes as work changes. Publish useful employee outputs as deliverables. Report outcomes and blockers only; the human does not need narration about expired employees, name releases, expiry timers, summons, integration mechanics, or other internal Boss operations. Speak when work completes, a timely interruption will help the human, the human needs to act, or they ask; stay quiet otherwise. Respect user-set resource constraints, including model routing and employee caps, and preserve them durably in memory. Build and maintain a reusable persona library across projects: notice recurring work patterns, create named purpose-specific roles such as Researcher, Feature Developer, Bug Investigator, or Verifier, and refine existing roles as experience accumulates. Inspect existing personas before adding one; improve a close match instead of creating duplicates. Keep each persona's guidance focused on durable methods and boundaries that transfer across projects. Maintain personas and your own files. Your memory is a standing duty: record durable facts and decisions in explicit named buckets, keep notes concise and scoped, and append corrections rather than rewriting history. You control all employees and personas.\n\nDispatch speed: when the human hands you a task, summon promptly — seconds, not minutes. Do not research the codebase before summoning. The only pre-summon research allowed is identifying which project the task belongs to when that is genuinely ambiguous. Write a competent brief and let the employee locate files, verify line numbers, and orient itself — that is what employees are for.\n\nEmployees assigned to a project automatically receive read and insert access to its shared project bucket. Grant additional Boss-created buckets deliberately through persona bucket IDs or per-employee permission overrides. Personal buckets remain Boss-only unless granted. Pinned files are documents only; they do not grant memory access; a per-field `permissions` object on summon — or `setPermissions` via control — tailors one employee's grants without editing the persona. Choose a purpose-specific jobTitle when summoning each employee; Goddard assigns their human name. Choose the employee's `icon` deliberately from the summon field — pick the icon that fits the actual work rather than leaving it to the job-title heuristic, and give jobs accurate titles since titles feed the fallback classifier.";

const BOSS_DEFAULT_V2: &str = "You coordinate employees for the human. Heavy delegation is your default: assign code changes, research, internet access, builds, code generation, long-running checks and tests, for daemon-managed worktrees, require each assigned employee to include 'agent-merge per rules', commit its unit, and run 'goddard-agent merge submit' itself; do not summon a separate Worktree Integrator. Employees report conflicts or verification failures with 'reportBlocker' only when supervisor or human action is required and report the landed SHA on success. Never run or poll long-running commands yourself. Ask employees to use shared build caches or dedicated output directories when that avoids contention with the user's tools. Never poll, watch, or wait yourself — hand recurring checks and waits to an employee; mark each summon `workGoal` — an `errand` when you need to know when it finishes (its finish reports back to you), a `goal` when it finishes without you (its record lands on the human's Goals page instead) — a finish also reaches you when the employee flagged a blocker or its session failed. Use `roster` for a cheap status check, `view` for employee details, and `context` for the user's work. Verify completion from the worktree and its commits before reporting work done; do not rely on a summary alone.\n\nUse `steer` for mid-flight corrections that change what the employee is writing right now — a steer that redirects the assignment itself may carry `jobTitle` to relabel the job. Use `prompt` for content whose relevance starts after the current step, such as queue additions or follow-ups. Prompt or steer can resume an employee after its idle expiry with the same transcript; summon a fresh employee for a new or distinct job, or when the previous employee is dead or finishing; never stack prompts onto an expiring employee, where queued work may be lost.\n\nTrack employee ownership, worktrees, and landed versus in-flight work in the appropriate memory bucket, and reconcile the notes as work changes. Publish employee outputs as deliverables only when the human explicitly or implicitly requested a report, document, or artifact. Keep routine investigations, audits, matrices, and JSON evidence in employee reports and transcripts. Apply this boundary to reusable personas. Write requested Markdown deliverables for a non-technical product designer, with plain language, a descriptive title, purpose, context, useful headings, and recommendations. Report outcomes and blockers only; the human does not need narration about expired employees, name releases, expiry timers, summons, integration mechanics, or other internal Boss operations. Speak when work completes, a timely interruption will help the human, the human needs to act, or they ask; stay quiet otherwise. Respect user-set resource constraints, including model routing and employee caps, and preserve them durably in memory. Build and maintain a reusable persona library across projects: notice recurring work patterns, create named purpose-specific roles such as Researcher, Feature Developer, Bug Investigator, or Verifier, and refine existing roles as experience accumulates. Inspect existing personas before adding one; improve a close match instead of creating duplicates. Keep each persona's guidance focused on durable methods and boundaries that transfer across projects. Maintain personas and your own files. Your memory is a standing duty: record durable facts and decisions in explicit named buckets, keep notes concise and scoped, and append corrections rather than rewriting history. You control all employees and personas.\n\nDispatch speed: when the human hands you a task, summon promptly — seconds, not minutes. Do not research the codebase before summoning. The only pre-summon research allowed is identifying which project the task belongs to when that is genuinely ambiguous. Write a competent brief and let the employee locate files, verify line numbers, and orient itself — that is what employees are for.\n\nEmployees assigned to a project automatically receive read and insert access to its shared project bucket. Grant additional Boss-created buckets deliberately through persona bucket IDs or per-employee permission overrides. Personal buckets remain Boss-only unless granted. Pinned files are documents only; they do not grant memory access; a per-field `permissions` object on summon — or `setPermissions` via control — tailors one employee's grants without editing the persona. Choose a purpose-specific jobTitle when summoning each employee; Goddard assigns their human name. Choose the employee's `icon` deliberately from the summon field — pick the icon that fits the actual work rather than leaving it to the job-title heuristic, and give jobs accurate titles since titles feed the fallback classifier.";

/// Revision 3: the refined coordinator — proactive persona maintenance,
/// outcome-aware assignment with `finishes` reserved for completing the
/// outcome, and memory duties shared with the runtime prompt.
const BOSS_DEFAULT_V3: &str = "Coordinate work for the human. Understand the requested outcome, then promptly assign execution to an appropriate employee. Give each assignment a clear purpose, scope, constraints, expected result, and relevant verification. Delegate research, implementation, integration, builds, tests, and monitoring; keep yourself available for the human.\n\nChoose a reusable custom persona when specialist guidance helps. Every employee automatically receives the Employee base, so custom personas should contain role-specific methods and limits. Assign the correct project and only the access the work requires. Use current user preferences and available capacity when choosing a provider and model.\n\nActively create and refine reusable personas as you learn what employees need to succeed. Treat repeated instructions, recurring corrections, and verified success patterns as candidates for lasting guidance. Inspect existing personas first; refine a matching role before creating a duplicate. Put responsibilities shared by all employees in Employee, specialist methods in the relevant custom role, and one-off requirements in the assignment. Keep guidance concise, reconcile contradictions, and remove obsolete repetition. Improve personas promptly within the human's standing authorization; do not wait to be asked. Preserve explicit human policies and access grants.\n\nUse memory actively within the access described by the shared runtime prompt. Retrieve relevant project decisions, preferences, and ongoing state before preparing assignments. Record durable coordination decisions and outcomes in the appropriate granted buckets. Turn reusable behavioral lessons into persona guidance; keep facts, preferences, project decisions, and supporting evidence in memory. Preserve the rationale for important persona refinements. Keep personal context compartmentalized and grant it to employees only when their work requires it.\n\nKeep ownership and outcomes clear. Tag every assignment with the human outcome it advances and the relevant work item when available. Include research, implementation, integration, verification, monitoring, and recovery so the Tasks tab represents the whole effort. State the assignment's completion condition and the next dependent step. Tag an assignment as finishes only when it is expected to complete the outcome, including required verification and integration. Other assignments advance the outcome and leave coordination with you. On completion, check the result and promptly advance authorized next steps without waiting for the human to remind you. Use a fresh employee for follow-up work when the previous one has finished. Route corrections affecting ongoing work immediately and queue later work only when delivery is reliable. Employees performing code changes own verification and the required integration flow. An outcome is complete only when its full acceptance conditions are met.\n\nPreserve user-owned work and avoid unrelated changes. Do not expand a task merely because a worthwhile improvement appears; distinguish optional recommendations from required work.\n\nReport concrete outcomes and material blockers concisely. Name employees when referring to their work. Verify completion with the relevant evidence before declaring success. Avoid routine interruptions; draw attention to decisions, failures, and results that need the human now.\n\nProject documentation belongs with the project under its conventions. Requested reports and other Boss-facing artifacts belong in deliverables. Do not publish internal research merely because it could be useful.";

const BOSS_PERSONA_REVISIONS: &[PersonaDefaultRevision] = &[
    PersonaDefaultRevision {
        revision: 1,
        markdown: BOSS_DEFAULT_V1,
    },
    PersonaDefaultRevision {
        revision: 2,
        markdown: BOSS_DEFAULT_V2,
    },
    PersonaDefaultRevision {
        revision: 3,
        markdown: BOSS_DEFAULT_V3,
    },
];

// Specialist role texts are additions to the Employee base, not
// replacements for it — they carry only the role's distinctive methods
// and limits.

/// Revision 1: the read-only investigation role — evidence with source
/// references, and a deliverable only when the assignment calls for one.
const RESEARCHER_DEFAULT_V1: &str = "Investigate the assigned question using read-only methods. Identify the evidence needed to answer it, consult relevant primary sources and granted project context, and distinguish confirmed facts from inference. Support consequential findings with precise source references. Report the useful conclusion, uncertainty, and any decision it informs. Do not modify the project. When the assignment is intended to answer the human's question or requests a report, publish a concise user-facing deliverable; keep supervisory reconnaissance in the supervisor report.";

const RESEARCHER_PERSONA_REVISIONS: &[PersonaDefaultRevision] = &[PersonaDefaultRevision {
    revision: 1,
    markdown: RESEARCHER_DEFAULT_V1,
}];

/// Revision 1: the implementation role — trace before editing, focused
/// changes, project-appropriate verification, framework feedback routed
/// through its documented mechanism.
const FEATURE_DEVELOPER_DEFAULT_V1: &str = "Implement the assigned change in the assigned project or workspace. Trace the relevant behavior and its dependencies before editing. Follow nearby patterns, keep the change focused, and verify the requested behavior with the project's appropriate checks. Report material defects in a consumed framework through its documented feedback mechanism, preserving the expected behavior and evidence.";

const FEATURE_DEVELOPER_PERSONA_REVISIONS: &[PersonaDefaultRevision] =
    &[PersonaDefaultRevision {
        revision: 1,
        markdown: FEATURE_DEVELOPER_DEFAULT_V1,
    }];

/// Revision 1: the failure-diagnosis role — evidence before editing, an
/// explicit account when reproduction is unavailable, and no fix unless
/// the assignment authorizes one.
const BUG_INVESTIGATOR_DEFAULT_V1: &str = "Investigate the assigned failure before changing code. Reproduce it when possible, trace the relevant path, and identify the root cause with evidence. If reproduction is unavailable, distinguish the evidence you have from the behavior you could not observe. Fix only when the assignment authorizes a fix; add regression coverage appropriate to the failure. If the cause remains ambiguous, report supported findings and the smallest next diagnostic step rather than guessing.";

const BUG_INVESTIGATOR_PERSONA_REVISIONS: &[PersonaDefaultRevision] =
    &[PersonaDefaultRevision {
        revision: 1,
        markdown: BUG_INVESTIGATOR_DEFAULT_V1,
    }];

/// Revision 1: the check-running role — no code edits, actual results
/// over claims, and a failed check distinguished from a broken
/// environment.
const VERIFIER_DEFAULT_V1: &str = "Run the assigned checks against the specified project, revision, and environment. Do not modify project code. Wait for required checks to complete and report what actually ran, what passed, what failed, and what could not be verified. Preserve useful error evidence, with file and line references when available. Distinguish a failed check from an environment or setup failure; do not claim behavior was verified from a build alone.";

const VERIFIER_PERSONA_REVISIONS: &[PersonaDefaultRevision] = &[PersonaDefaultRevision {
    revision: 1,
    markdown: VERIFIER_DEFAULT_V1,
}];

/// Every shipped revision of a canonical default, oldest first — the last
/// entry is the text this build installs.
pub fn shipped_persona_revisions(role: PersonaDefaultRole) -> &'static [PersonaDefaultRevision] {
    match role {
        PersonaDefaultRole::Boss => BOSS_PERSONA_REVISIONS,
        PersonaDefaultRole::Employee => EMPLOYEE_PERSONA_REVISIONS,
        PersonaDefaultRole::Researcher => RESEARCHER_PERSONA_REVISIONS,
        PersonaDefaultRole::FeatureDeveloper => FEATURE_DEVELOPER_PERSONA_REVISIONS,
        PersonaDefaultRole::BugInvestigator => BUG_INVESTIGATOR_PERSONA_REVISIONS,
        PersonaDefaultRole::Verifier => VERIFIER_PERSONA_REVISIONS,
    }
}

/// The shipped default this build installs for the role.
pub fn shipped_persona_default(role: PersonaDefaultRole) -> &'static PersonaDefaultRevision {
    shipped_persona_revisions(role)
        .last()
        .expect("persona default catalogs are never empty")
}

/// The newest shipped revision whose text byte-matches `markdown` — a
/// saved persona with a match demonstrably carries no custom edits.
pub fn shipped_persona_revision(role: PersonaDefaultRole, markdown: &str) -> Option<u32> {
    shipped_persona_revisions(role)
        .iter()
        .rev()
        .find(|entry| entry.markdown == markdown)
        .map(|entry| entry.revision)
}

/// A `@@`-grouped unified line diff between two instruction texts —
/// `-`/`+`/space markers with three context lines. `None` when identical.
pub fn instruction_diff(old: &str, new: &str) -> Option<String> {
    if old == new {
        return None;
    }
    let old_lines = old.lines().collect::<Vec<_>>();
    let new_lines = new.lines().collect::<Vec<_>>();
    let diff = similar::TextDiff::from_slices(&old_lines, &new_lines);
    let mut text = String::new();
    for group in diff.grouped_ops(3) {
        text.push_str("@@\n");
        for op in &group {
            for change in diff.iter_changes(op) {
                text.push(match change.tag() {
                    similar::ChangeTag::Equal => ' ',
                    similar::ChangeTag::Delete => '-',
                    similar::ChangeTag::Insert => '+',
                });
                text.push_str(change.value());
                text.push('\n');
            }
        }
    }
    Some(text)
}

/// Revision provenance for one canonical default persona. Persisted in
/// [`BossState`] so untouched detection, review acknowledgment, undo, and
/// pending notices all survive restarts.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct PersonaDefaultState {
    /// The shipped revision the saved instructions demonstrably began
    /// from — `None` marks text of unknown origin, which never compares
    /// as untouched and never adopts automatically.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub starting_revision: Option<u32>,
    /// The newest shipped revision the human acknowledged — keeping the
    /// current text, approving a proposal, or resetting. Kept separate
    /// from `starting_revision`: reviewing an update never claims its
    /// instructions were adopted, and a partial incorporation still reads
    /// as customized.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reviewed_revision: Option<u32>,
    /// The newest shipped revision this installation has loaded — the
    /// upgrade detector's baseline. First run stamps the current
    /// revision; a later bump against it is an upgrade event. An older
    /// installed version never rolls it back, so a downgrade cannot
    /// replace a newer saved baseline.
    pub seen_revision: u32,
    /// The instructions a reset or approved adoption replaced —
    /// recoverable until another edit lands.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub undo: Option<PersonaDefaultUndo>,
    /// A boss-drafted instruction update awaiting the human's decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub proposal: Option<PersonaDefaultProposal>,
}

/// The recoverable half of a reset or approved adoption.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PersonaDefaultUndo {
    /// The instructions the change replaced.
    pub markdown: String,
    /// The starting revision that text carried — restored with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub starting_revision: Option<u32>,
    /// The reviewed-revision watermark before the change — restored with
    /// the text so undoing an adoption reopens the review it resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reviewed_revision: Option<u32>,
    /// The text the change wrote. Undo applies only while the saved
    /// instructions still equal it; a later edit turns undo into a
    /// reviewable restore rather than a silent overwrite.
    pub applied_markdown: String,
}

/// A complete proposed replacement drafted by the boss for human review.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PersonaDefaultProposal {
    /// The proposed instructions in full.
    pub markdown: String,
    /// The saved text the proposal was drafted against — drift between it
    /// and the current saved text marks the proposal stale so approval
    /// refreshes rather than overwriting intervening edits.
    pub baseline_markdown: String,
    /// The shipped revision the proposal incorporates.
    pub target_revision: u32,
    pub created_at: u64,
}

/// One role's line in the consolidated upgrade notice.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PersonaDefaultNoticeEntry {
    pub role: PersonaDefaultRole,
    /// The shipped revision the entry reports.
    pub revision: u32,
    /// True when untouched instructions adopted the revision
    /// automatically — the notice reports the change; false means the
    /// saved text is customized or unknown and awaits review.
    pub adopted: bool,
}

/// The consolidated persona-defaults upgrade notice, delivered once into
/// the boss's next natural turn and persisted until then. Entries stay
/// after delivery — delivered, not adopted — so the settings surface can
/// keep pending reviews discoverable without re-notifying each turn.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PersonaDefaultNotice {
    pub updates: Vec<PersonaDefaultNoticeEntry>,
    pub delivered: bool,
}

/// Every canonical default's revision provenance — the base roles plus
/// one entry per shipped specialist.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct PersonaDefaultsState {
    pub boss: PersonaDefaultState,
    pub employee: PersonaDefaultState,
    pub researcher: PersonaDefaultState,
    pub feature_developer: PersonaDefaultState,
    pub bug_investigator: PersonaDefaultState,
    pub verifier: PersonaDefaultState,
}

impl PersonaDefaultsState {
    pub fn get(&self, role: PersonaDefaultRole) -> &PersonaDefaultState {
        match role {
            PersonaDefaultRole::Boss => &self.boss,
            PersonaDefaultRole::Employee => &self.employee,
            PersonaDefaultRole::Researcher => &self.researcher,
            PersonaDefaultRole::FeatureDeveloper => &self.feature_developer,
            PersonaDefaultRole::BugInvestigator => &self.bug_investigator,
            PersonaDefaultRole::Verifier => &self.verifier,
        }
    }

    pub fn get_mut(&mut self, role: PersonaDefaultRole) -> &mut PersonaDefaultState {
        match role {
            PersonaDefaultRole::Boss => &mut self.boss,
            PersonaDefaultRole::Employee => &mut self.employee,
            PersonaDefaultRole::Researcher => &mut self.researcher,
            PersonaDefaultRole::FeatureDeveloper => &mut self.feature_developer,
            PersonaDefaultRole::BugInvestigator => &mut self.bug_investigator,
            PersonaDefaultRole::Verifier => &mut self.verifier,
        }
    }
}

/// One step in the shipped-default lifecycle — inspect, reset, undo,
/// keep, propose, adopt, dismiss. See [`BossOperation::PersonaDefault`].
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PersonaDefaultAction {
    /// Full shipped texts, revision labels, saved provenance, and ready
    /// comparisons for both defaults — read-only, no side effects.
    Inspect,
    /// Replace the role's saved instructions with the latest shipped
    /// default — instructions only; name, permissions, pinned documents,
    /// and identity stay. The replaced text stays recoverable through
    /// `undo` until another edit lands. No-op bails when already latest.
    Reset { role: PersonaDefaultRole },
    /// Restore the instructions a reset or adoption replaced. Applies
    /// only while the saved text still equals what that action wrote —
    /// a later edit fails the restore rather than overwriting it.
    Undo { role: PersonaDefaultRole },
    /// Keep the current instructions and mark the latest shipped revision
    /// reviewed — suppresses its pending notice and any open proposal
    /// without adopting it. Human-only: customized text changes and
    /// acknowledgments are the human's call.
    Keep { role: PersonaDefaultRole },
    /// Store a boss-drafted proposal for the human's review — the saved
    /// text it was written against is recorded so later edits mark the
    /// proposal stale instead of being overwritten.
    Propose {
        role: PersonaDefaultRole,
        markdown: String,
    },
    /// Human-approved incorporation: writes `markdown` as the saved
    /// instructions and marks the latest shipped revision reviewed.
    /// `expected_saved`, when given, must equal the current saved text —
    /// a mismatch means edits landed since review and the write refuses
    /// rather than discarding them. Human-only.
    Adopt {
        role: PersonaDefaultRole,
        markdown: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        expected_saved: Option<String>,
    },
    /// Drop an open proposal without deciding on the update.
    DismissProposal { role: PersonaDefaultRole },
    /// Record which saved persona is the canonical Employee base — the
    /// resolution for a legacy document whose stored marker is missing
    /// and whose candidates identity cannot decide. Every candidate
    /// record stays intact; only the marker moves. Human-only: choosing
    /// the base is the human's call, never a daemon or boss guess.
    ChooseEmployeeBase { persona_id: Uuid },
}

/// One canonical default's full inspection record — shipped text,
/// revision labels, saved provenance, and computed comparisons.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PersonaDefaultInfo {
    pub role: PersonaDefaultRole,
    /// The canonical persona record — `None` when the role's persona is
    /// missing, which the surface reports as an actionable error.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub persona_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub saved_markdown: Option<String>,
    /// The saved text byte-matches the latest shipped instructions.
    pub using_latest: bool,
    /// The saved text byte-matches some shipped revision — provably free
    /// of custom edits, so upgrades may adopt the latest automatically.
    pub untouched: bool,
    /// The revision the saved text demonstrably started from — `None`
    /// reads as "starting revision unknown," and no difference below may
    /// be labeled a user addition.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub starting_revision: Option<u32>,
    /// The shipped text at `starting_revision`, for telling shipped
    /// changes apart from custom edits.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub starting_markdown: Option<String>,
    /// The newest shipped revision the human acknowledged.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reviewed_revision: Option<u32>,
    /// The newest shipped revision this installation has loaded.
    pub seen_revision: u32,
    pub shipped_revision: u32,
    pub shipped_markdown: String,
    /// Saved → latest-shipped unified diff; `None` when identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub saved_diff: Option<String>,
    /// Shipped(starting) → shipped(latest) unified diff — the changes
    /// Goddard introduced between the saved text's baseline and now.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub shipped_diff: Option<String>,
    /// A pending upgrade-notice entry still names this role for review.
    pub update_pending: bool,
    /// The recoverable pre-change instructions, when recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub undo: Option<PersonaDefaultUndo>,
    /// Undo can restore directly — false means later edits landed and a
    /// restore must go through review.
    pub undo_applies: bool,
    /// The proposal awaiting the human's decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub proposal: Option<PersonaDefaultProposal>,
    /// The saved text moved on since the proposal's baseline — approving
    /// it would overwrite intervening edits, so refresh instead.
    pub proposal_stale: bool,
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

/// Where an employee sits in the summon queue's admission lifecycle.
/// `dispatching` and `finishing` are explicit accounting states: the
/// daemon persists them so capacity is released exactly once, after the
/// runtime's termination is known rather than while teardown is a race
/// window. Records written before the queue existed deserialize as
/// `working`; `BossEmployee::expired` remains the wire-compat projection
/// (`state == expired`).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EmployeeLifecycle {
    /// Accepted but not launched — holds zero capacity: no model slot,
    /// no worktree, no runtime, no device claims.
    Queued,
    /// Granted a model slot and any declared host resources; the launch
    /// intent is persisted but the provider may not have started yet.
    Dispatching,
    /// The assignment reached the provider — or the employee is an idle
    /// admitted worker holding its slot for the next prompt.
    #[default]
    Working,
    /// Teardown begun; the model slot stays held until runtime
    /// termination settles the release.
    Finishing,
    /// Done — the roster keeps the record for reuse until retirement.
    Expired,
}

/// What settled the employee's admission — a runtime signal the daemon
/// reports to the Boss service so the finish can classify the expiry
/// legibly. The durable record is `ExpiryCause`; this is only how the
/// settle was observed.
#[derive(Clone, Copy, Debug)]
pub enum EmployeeSettle {
    /// The provider's turn ended — success or failure is the session's
    /// terminal verdict, read at finish time.
    TurnFinished,
    /// The provider process exited; `mid_turn` records whether a turn
    /// was still open when it died.
    ProcessExited { mid_turn: bool },
    /// A supervisor stopped or cancelled the employee.
    Stopped,
    /// Dispatch could not deliver the assignment — the launch failed.
    LaunchFailed,
    /// A daemon restart forced the finish.
    Restarted,
}

/// Why an employee's last admission ended — the settle classification
/// written on the expired record so an interruption reads as, say,
/// "interrupted — provider exited mid-turn" rather than a bare dead row.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExpiryCause {
    /// The turn finished with nothing parked and no ask open.
    Finished,
    /// The turn or session failed, or launch could not deliver the job.
    Failed,
    /// The provider process exited while a turn was open.
    ExitedMidTurn,
    /// The provider process exited while the employee held no turn.
    ExitedIdle,
    /// A clean settle left agent-owned prompts parked in the queue.
    ParkedWork,
    /// A clean settle left an `agentAsk` question unanswered.
    UnansweredAsk,
    /// A supervisor stopped or cancelled the employee — terminal intent.
    Stopped,
    /// A daemon restart forced the finish.
    Restarted,
}

impl ExpiryCause {
    /// Classify an expiry from the settle signal plus what the finish
    /// pass found parked or unanswered. Leftovers upgrade only a
    /// otherwise-clean settle — a failed turn keeps its cause and the
    /// parked count still lands on the record.
    pub fn for_settle(
        settle: EmployeeSettle,
        failed: bool,
        parked_prompts: u32,
        pending_question: bool,
    ) -> Self {
        match settle {
            EmployeeSettle::Stopped => Self::Stopped,
            EmployeeSettle::Restarted => Self::Restarted,
            EmployeeSettle::LaunchFailed => Self::Failed,
            EmployeeSettle::ProcessExited { mid_turn: true } => Self::ExitedMidTurn,
            EmployeeSettle::ProcessExited { mid_turn: false } => Self::ExitedIdle,
            EmployeeSettle::TurnFinished if failed => Self::Failed,
            EmployeeSettle::TurnFinished if pending_question => Self::UnansweredAsk,
            EmployeeSettle::TurnFinished if parked_prompts > 0 => Self::ParkedWork,
            EmployeeSettle::TurnFinished => Self::Finished,
        }
    }

    /// Whether reviving the job makes sense — every expiry revives
    /// through `resume` or a plain prompt, a supervisor stop included:
    /// the stop is terminal only for the admission it ended.
    pub fn resumable(self) -> bool {
        true
    }

    /// Whether the settle cut live work off — the wave tally counts it
    /// as a failure rather than a finish.
    pub fn interrupted(self) -> bool {
        matches!(self, Self::Failed | Self::ExitedMidTurn | Self::Restarted)
    }

    /// Whether the settle appends to the ticket's interruption history —
    /// anything but a clean finish or a deliberate stop counts, so a
    /// crash-looping employee reports "Nth interruption" honestly.
    pub fn counts_interruption(self) -> bool {
        !matches!(self, Self::Finished | Self::Stopped)
    }

    /// Whether the cause alone warrants an interruption report — parked
    /// prompts and an unanswered ask still report through
    /// [`EmployeeExpiry::reports`], and an idle exit reports only with
    /// leftovers.
    pub fn reports(self) -> bool {
        matches!(
            self,
            Self::Failed
                | Self::ExitedMidTurn
                | Self::Restarted
                | Self::ParkedWork
                | Self::UnansweredAsk
        )
    }

    /// The transcript notice an interrupted expiry leaves — `None` for
    /// settles that already write their own record (a failed turn's
    /// summary row, a supervisor's stop) and for finishes.
    pub fn notice(self) -> Option<&'static str> {
        match self {
            Self::Restarted => Some("Turn interrupted — the daemon restarted"),
            Self::ExitedMidTurn => Some("Turn interrupted — the provider process exited"),
            Self::ExitedIdle => Some("The provider process exited while the employee was idle"),
            _ => None,
        }
    }

    /// The wire name — digests and logs render the cause compactly.
    pub fn label(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::ExitedMidTurn => "exited-mid-turn",
            Self::ExitedIdle => "exited-idle",
            Self::ParkedWork => "parked-work",
            Self::UnansweredAsk => "unanswered-ask",
            Self::Stopped => "stopped",
            Self::Restarted => "restarted",
        }
    }

    /// The short cause phrase reports and resume prompts embed.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Failed => "a failed turn",
            Self::ExitedMidTurn => "the provider process exited mid-turn",
            Self::ExitedIdle => "the provider process exited while idle",
            Self::ParkedWork => "prompts stayed parked in its queue",
            Self::UnansweredAsk => "a question went unanswered",
            Self::Stopped => "a supervisor stopped it",
            Self::Restarted => "a daemon restart",
        }
    }
}

/// The settle record an expired employee carries — the cause, whether
/// reviving makes sense, and the leftovers (parked prompts, an
/// unanswered ask) the finish surfaces so they are never silently lost.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EmployeeExpiry {
    pub cause: ExpiryCause,
    /// Whether reviving the job makes sense — `cause.resumable()`
    /// copied onto the wire so clients never re-derive it.
    pub resumable: bool,
    /// Agent-owned prompts still parked in the mirrored queue at expiry
    /// — they ride the ticket's pending list ahead of a revive prompt.
    #[serde(default)]
    pub parked_prompts: u32,
    /// The `agentAsk` text still unanswered when the job expired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pending_question: Option<String>,
}

impl EmployeeExpiry {
    /// The record at `begin_finishing` time — leftovers are filled in by
    /// the finish tail once the session document is readable.
    pub fn settle(cause: ExpiryCause) -> Self {
        Self {
            cause,
            resumable: cause.resumable(),
            parked_prompts: 0,
            pending_question: None,
        }
    }

    /// Whether the expiry warrants a report beyond the work kind's own
    /// contract — interruptions and unfinished leftovers (parked
    /// prompts, an unanswered ask) report; plain finishes and stops do
    /// not.
    pub fn reports(&self) -> bool {
        self.cause.reports() || self.parked_prompts > 0 || self.pending_question.is_some()
    }
}

/// One interruption a ticket's admission settled with — the boss's
/// "Nth interruption" bookkeeping, capped so a crash loop stays legible
/// without growing the record.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InterruptionRecord {
    pub cause: ExpiryCause,
    pub at: u64,
}

/// The interruption history a ticket retains — appended at expiry,
/// oldest entries drop past the cap.
pub const INTERRUPTION_HISTORY_CAP: usize = 16;

/// The durable admission ticket a summon persists before it returns.
/// Everything the deferred launch needs lives here so a restart re-creates
/// the pending admission idempotently rather than re-asking the boss.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SummonTicket {
    /// Queue order across the daemon — monotonic, never reused. The boss's
    /// oldest queued sequence is the only entry that may dispatch.
    pub sequence: u64,
    /// Admission generation — bumped each time the employee re-enters
    /// admission (resurrection, setModel requeue). Launch completion
    /// settles against it so a concurrent stop or model change cannot
    /// start stale work.
    pub generation: u64,
    /// The resolved provider and concrete model id the ticket counts
    /// against — routing ran at admission, never at dispatch.
    pub provider: ProviderKind,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reasoning_effort: Option<String>,
    /// The assignment as the summoner wrote it — dispatch resolves the
    /// Boss employee wrapper and appends `pending_prompts` in order.
    pub prompt: String,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub workspace: Option<AgentWorkspace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub base_branch: Option<String>,
    /// The daemon-managed worktree a `workspace: "adopt"` ticket takes
    /// over. `None` on every other workspace kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    #[ts(type = "string")]
    pub adopt_worktree: Option<PathBuf>,
    /// Host resources the summon declared for the assignment's lifetime —
    /// claimed atomically with the model slot at dispatch.
    #[serde(default)]
    pub resources: crate::resources::ResourceSet,
    /// The summon may spend burst slots above `liveLimit` up to
    /// `hardCap`; without it admission stops at `liveLimit`.
    #[serde(default)]
    pub allow_burst: bool,
    /// Prompts parked while the ticket waits — they join the dispatch
    /// envelope in submission order and survive restart.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_prompts: Vec<String>,
    /// Optional wave/group id — every ticket admitted under the same id
    /// is a wave member; the daemon notifies the supervisor once when
    /// the membership goes all-terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group_id: Option<String>,
    /// Optional dispatch priority — reserved for the same phase; dispatch
    /// remains strict FIFO today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub priority: Option<i64>,
    /// The daemon-owned [`BossOutcome`] this assignment serves as an
    /// assignment, when the summoner linked one — the same link
    /// `BossEmployee::assignment` carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub outcome_id: Option<Uuid>,
    /// The broker reservation holding this admission's claims once
    /// granted — released exactly once when the generation expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reservation: Option<Uuid>,
    /// A resource-set change parked on a working employee — the
    /// scheduler retries admission for this set under
    /// `pending_reservation` and swaps it in on grant; until then the
    /// employee's current claims stand. Only a working employee carries
    /// one: queued tickets edit `resources` directly, and a requeue
    /// folds a parked set into `resources` so the fresh admission claims
    /// it wholesale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pending_resources: Option<crate::resources::ResourceSet>,
    /// The reservation id the parked set's admission retries under —
    /// stable per request so a lost response or restart re-issues rather
    /// than double-claiming, and distinct from `reservation` so the swap
    /// can tell the two claims apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pending_reservation: Option<Uuid>,
    /// Why the head-of-line entry is still waiting — refreshed by the
    /// scheduler, empty while a dispatch path exists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<AdmissionBlocker>,
    /// The outbox event id recorded for this generation's dispatch
    /// notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dispatch_event: Option<u64>,
    /// The interruption history this ticket's admissions have settled
    /// with — appended at expiry for every cause that counts, capped at
    /// [`INTERRUPTION_HISTORY_CAP`] with the oldest dropped. Advisory
    /// bookkeeping so the boss can read "Nth interruption" rather than a
    /// daemon policy; it never gates recovery.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interruptions: Vec<InterruptionRecord>,
    /// How many times this ticket was resumed after an interruption —
    /// the boss's read on whether the job is stuck in a crash loop.
    #[serde(default)]
    pub resume_count: u32,
    /// The settle cause the latest resume answered — None until the
    /// ticket has been resumed once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_resumed_cause: Option<ExpiryCause>,
}

/// Why an accepted ticket cannot dispatch yet — a wait reason, never an
/// RPC error.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AdmissionBlocker {
    /// The provider+model cap is full.
    ModelLimit { used: u32, limit: u32 },
    /// A declared host resource set cannot be granted right now.
    HostResources { detail: String },
    /// A linked assignment's readiness has not landed — the finishing
    /// assignment waits on the outcome's completion conditions, an
    /// ordinary one on its prerequisites.
    OutcomeWait { detail: String },
    /// The canonical Employee base is unresolved — a human choice in
    /// the Personas settings clears it.
    EmployeeBase { detail: String },
}

/// Admission status attached to `Summoned` and control results — the
/// resolved selection plus, while queued, the position and wait reasons.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SummonAdmission {
    pub provider: ProviderKind,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub queue_position: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<AdmissionBlocker>,
}

/// One provider+model rule in the boss-set resource policy. `liveLimit`
/// is the normal cap; `hardCap` is reachable only by summons that carry
/// `allowBurst`. `0 <= liveLimit <= hardCap`; an explicit zero pauses the
/// model's queue and a missing rule imposes no model cap.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelLimit {
    pub provider: ProviderKind,
    pub model: String,
    pub live_limit: u32,
    pub hard_cap: u32,
}

/// The boss-configured admission policy — daemon state the boss never has
/// to remember in its persona. `revision` increments on each accepted
/// `setResourcePolicy` and guards lost-update races through
/// `expectedRevision`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct BossResourcePolicy {
    pub revision: u64,
    pub model_limits: Vec<ModelLimit>,
    /// Desired host broker policy — reapplied to the host broker every
    /// time the Boss service activates, so a daemon restart reconciles a
    /// policy write that crashed between the two stores.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub host: Option<crate::resources::ResourcePolicy>,
}

/// A dispatch notification the daemon owes a supervisor — durable so a
/// restart neither drops nor duplicates it. Delivery dedupes on `id`.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DispatchNotification {
    pub id: u64,
    pub session_id: Uuid,
    pub generation: u64,
    pub provider: ProviderKind,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub outcome_id: Option<Uuid>,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub delivered_at: Option<u64>,
}

/// How a wave member's current admission ended — recorded when the
/// employee commits to terminal, cleared if it re-enters the queue.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WaveMemberOutcome {
    /// The assignment settled — a clean or idle expiry.
    Finished,
    /// The member flagged a blocker, its session failed, or the launch
    /// or restart recovery could not deliver the assignment.
    Failed,
    /// A supervisor stopped the employee before it finished.
    Cancelled,
}

/// One employee admitted under a wave id. `outcome` is `None` while the
/// member is in flight — queued, dispatching, or working.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WaveMember {
    pub session_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub outcome: Option<WaveMemberOutcome>,
}

/// A summon group (`groupId`) the daemon watches for an all-terminal
/// membership. Each resolution pushes exactly one durable outbox
/// notification; admitting or reviving a member reopens the wave, and the
/// enlarged membership resolves once more.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossWave {
    /// The `groupId` the summoner passed, namespaced to this boss.
    pub id: String,
    /// The first member's supervisor — notification delivery escalates
    /// to the boss session when it cannot take prompts.
    pub supervisor_id: Uuid,
    /// Every employee ever admitted under this id — members are never
    /// removed, so retirement cannot strand the tally.
    pub members: Vec<WaveMember>,
    /// When the membership last went all-terminal — `None` while members
    /// are in flight or the wave has never resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolved_at: Option<u64>,
}

/// A wave-resolution notice the daemon owes a supervisor — the same
/// durable, id-deduped outbox contract `DispatchNotification` carries.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WaveNotification {
    pub id: u64,
    pub wave_id: String,
    /// The wave's supervisor at creation — resolved through the usual
    /// report-target fallback at delivery.
    pub supervisor_id: Uuid,
    /// Member tallies at resolution. `cancelled` counts separately so an
    /// all-cancelled wave reports as cancelled rather than clean.
    pub finished: u32,
    /// Blocker-flagged, session-failed, launch-failed, and
    /// restart-interrupted members.
    pub failed: u32,
    pub cancelled: u32,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub delivered_at: Option<u64>,
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
    /// A workspace move is retiring its old runtime. The admission stays
    /// live; normal turn settlement is suppressed until the move completes.
    /// A daemon restart resolves an abandoned move as an interruption.
    #[serde(default)]
    pub workspace_transition: bool,
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
    /// A supervisor stop marked the record before it expired — wave
    /// tallies count the member as cancelled rather than finished.
    /// Re-admission clears it like `blocker`.
    #[serde(default)]
    pub cancelled: bool,
    /// The settle classification from the last expiry — the
    /// interruption's cause, resumability, and leftovers the finish
    /// surfaced. `None` while live and on records that expired before
    /// the field existed; re-admission clears it with `blocker`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expiry: Option<EmployeeExpiry>,
    /// Admission lifecycle — `queued`, `dispatching`, `working`,
    /// `finishing`, or `expired`. Records written before the queue
    /// deserialize as `working`; `expired` stays the wire projection.
    #[serde(default)]
    pub state: EmployeeLifecycle,
    /// The employee's admission ticket — present from a queued summon
    /// through dispatch, and retained afterward for accounting and
    /// re-admission. Legacy employees predate it and run uncapped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ticket: Option<SummonTicket>,
    /// When the current queued stint began — `None` once dispatched or
    /// for employees that never queued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub queued_at: Option<u64>,
    /// Idempotency key the summoner supplied; a retry with the same
    /// fields returns this record instead of a second employee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub request_id: Option<Uuid>,
    /// Canonical fingerprint of the summon fields `request_id` covers —
    /// reusing the id with different fields is an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub request_fingerprint: Option<String>,
    /// The plan this assignment serves — a `BossPlan::id`. Set at summon
    /// or re-tagged through `control`'s `setPlan`; `None` lists the
    /// employee outside every plan group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plan_id: Option<Uuid>,
    /// The `PlanItem::id` inside `plan_id` the assignment serves — `None`
    /// leaves the employee unallocated at the bottom of its plan group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub item_id: Option<Uuid>,
    /// The task this assignment serves as a assignment of, plus the completion
    /// behavior captured at summon. `None` makes the employee a
    /// standalone job — its finish follows `work_goal` alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub assignment: Option<Assignment>,
}

/// An employee's membership in a [`BossOutcome`] — the assignment identity plus
/// the completion behavior fixed when the assignment started. A resume
/// is a new attempt under the same assignment; it inherits this capture
/// unchanged. Changing it means stopping and reassigning the work.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    /// The `BossOutcome::id` the assignment serves.
    pub outcome_id: Uuid,
    /// What the boss should decide or do with the result — delivered
    /// with the finish report and kept on the durable handoff. The
    /// summon supplies it for ordinary assignments; the visible default is
    /// [`DEFAULT_AFTER_SUCCESS`]. Finishing assignments carry no intent — a
    /// `finishesOutcome` summon with `afterSuccess` fails.
    #[serde(default)]
    pub after_success: String,
    /// The task's designated finisher: a successful finish completes the
    /// task without notifying the boss, provided the completion
    /// conditions still hold at acceptance.
    #[serde(default)]
    pub finishes_outcome: bool,
    /// Sibling assignments (employee session ids, same task) whose accepted
    /// success must land before this assignment starts. Readiness is
    /// admission, not ordering: a waiting assignment holds its slot until
    /// every prerequisite succeeds. One level, so a fresh leaf cannot
    /// close a cycle — but a cancelled prerequisite leaves the wait
    /// standing until the boss replaces it with a new assignment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<Uuid>,
}

/// The follow-up intent an ordinary assignment carries when its summon names
/// none — keeps every success actionable without inventing intent.
pub const DEFAULT_AFTER_SUCCESS: &str = "Review the result and decide the next step.";

impl BossEmployee {
    /// Move the employee to `lifecycle`, keeping the `expired` wire
    /// projection and its timestamp in step.
    pub fn set_lifecycle(&mut self, lifecycle: EmployeeLifecycle, now: u64) {
        self.state = lifecycle;
        self.expired = lifecycle == EmployeeLifecycle::Expired;
        if lifecycle != EmployeeLifecycle::Working {
            self.workspace_transition = false;
        }
        if self.expired && self.expired_at.is_none() {
            self.expired_at = Some(now);
        }
    }

    /// The lifecycle derived for records too old to carry `state`.
    pub fn lifecycle(&self) -> EmployeeLifecycle {
        if self.expired && self.state != EmployeeLifecycle::Expired {
            return EmployeeLifecycle::Expired;
        }
        self.state
    }
}

/// A file or folder of employee output the boss published to the user's
/// sidebar. `path` is absolute on the daemon's host and points at readable
/// bytes — publish copies the source into the daemon's deliverable store,
/// so the record survives the workspace it was made in. A `reference`
/// publish skips the copy and `path` names the live source instead.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossDeliverable {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    /// Where a copied deliverable was published from — the re-publish key
    /// and the provenance the sidebar shows. `None` marks a live-path
    /// reference, where `path` is itself the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_path: Option<String>,
    /// Recorded at publish time so renderers never stat the filesystem.
    pub directory: bool,
    pub created_at: u64,
    /// Re-publishing a path bumps this; the sidebar's recency window reads it.
    pub updated_at: u64,
    /// A pinned deliverable keeps its sidebar row past the recency window.
    #[serde(default)]
    pub pinned_at: Option<u64>,
    /// A swept deliverable hides behind the group's dormant fold until restored.
    #[serde(default)]
    pub dormant_at: Option<u64>,
    /// An archived deliverable leaves the sidebar entirely.
    #[serde(default)]
    pub archived_at: Option<u64>,
    /// When the user last opened the deliverable. `None` — or older than
    /// `updated_at` after a re-publish — reads as unread in the sidebar.
    #[serde(default)]
    pub viewed_at: Option<u64>,
}

/// Who moved a plan or one of its work items — the audit trail's actor.
/// Employee callers never reach the bookkeeping channel, so every caller
/// resolves to the boss or the user.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlanActor {
    Boss,
    User,
}

/// The durable outcome a plan sits in once the user approved it — set by
/// an explicit, audited action, never derived. `Approved` doubles as the
/// reopen target: a completed or abandoned plan returns to it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlanOutcome {
    Approved,
    Completed,
    Abandoned,
}

impl PlanOutcome {
    pub fn terminal(&self) -> bool {
        !matches!(self, PlanOutcome::Approved)
    }

    pub fn label(&self) -> &'static str {
        match self {
            PlanOutcome::Approved => "approved",
            PlanOutcome::Completed => "completed",
            PlanOutcome::Abandoned => "abandoned",
        }
    }
}

/// One entry in a plan's audit trail — the outcome a transition entered,
/// when, and who moved it. A `Completed`/`Abandoned` followed by
/// `Approved` is a reopen.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanTransition {
    pub outcome: PlanOutcome,
    pub at: u64,
    pub actor: PlanActor,
}

/// The stored state of a work-breakdown item — everything except
/// in-progress, which the panel derives from linked live employees and
/// the record never carries. `toDo` is also the reopen target: a done or
/// dropped item returns to it.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlanItemState {
    /// Declared, no linked live work.
    #[default]
    ToDo,
    /// Checked off — a claim that the item's intent was satisfied.
    Done,
    /// Removed from the course but kept visible as struck.
    Dropped,
}

impl PlanItemState {
    pub fn label(&self) -> &'static str {
        match self {
            PlanItemState::ToDo => "to do",
            PlanItemState::Done => "done",
            PlanItemState::Dropped => "dropped",
        }
    }
}

/// An audited item state change — a check-off, a drop, or a reopen back
/// to `toDo` — with actor and time.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanItemTransition {
    pub state: PlanItemState,
    pub at: u64,
    pub actor: PlanActor,
}

/// One step in a plan's ordered work breakdown, declared at dispatch
/// granularity — roughly one employee-job each. `id` is the stable
/// identity summon item tags and `setPlanItemState` target; the entry's
/// position in `BossPlan::items` is the declared course, never computed.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub state: PlanItemState,
    /// Every explicit state change in order. Adds, renames, and reorders
    /// are not lifecycle events and leave no entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<PlanItemTransition>,
}

/// An item's panel-facing state — the stored state plus `InProgress`,
/// derived from linked live employees and never persisted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanItemProgress {
    ToDo,
    InProgress,
    Done,
    Dropped,
}

/// One entry of an `updatePlanItems` payload — an entry naming an
/// existing item `id` renames and repositions it; an entry without one
/// creates a fresh `toDo` item.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanItemInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<Uuid>,
    pub title: String,
}

/// A planning session the boss opened with `createPlan`: the managed task
/// it drafts in and the plan document that task owns. `plan_file` is
/// relative to the Boss files root — always `plans/<name>.md`.
/// `finalized_at` freezes the document once the user approves
/// `finalizePlan`; the daemon archives the session after a grace period.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossPlan {
    /// The record's stable identity — employee `planId` links and plan
    /// references resolve to it. It survives plan-file renames and the
    /// planning session's archive; records predating the field mint one
    /// on load.
    #[serde(default = "new_plan_id")]
    pub id: Uuid,
    pub session_id: Uuid,
    pub plan_file: String,
    /// What the session is planning — the session's title carries the same
    /// text so the sidebar row reads as the idea.
    pub idea: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<u64>,
    /// The ordered work breakdown — the plan's intended course of work,
    /// seeded from the approved design at finalization and reshaped by the
    /// boss thereafter as execution diverges. The frozen document stays
    /// the baseline; this list is the live execution map.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<PlanItem>,
    /// The durable outcome — `None` on drafts and on finalized records
    /// predating the field, both of which read as open. Access through
    /// [`BossPlan::outcome`]/[`BossPlan::lifecycle`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<PlanOutcome>,
    /// The audited lifecycle trail — approval, closures, and reopens in
    /// order, each with actor and time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<PlanTransition>,
}

fn new_plan_id() -> Uuid {
    Uuid::new_v4()
}

/// The plan's panel-facing lifecycle — `Planning` stays derived from
/// `finalized_at` (an archived draft adds the session's archive status on
/// the client); the other states are the stored outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanLifecycle {
    Planning,
    Approved,
    Completed,
    Abandoned,
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

    /// The current outcome — finalized records predating the field read
    /// as approved, the state their freeze implied.
    pub fn outcome(&self) -> PlanOutcome {
        self.outcome.unwrap_or(PlanOutcome::Approved)
    }

    /// The lifecycle a viewer renders: a draft is still in planning, an
    /// approved plan is open, and a marked plan is completed or abandoned.
    /// An archived draft keeps `Planning` here — the client's session
    /// record carries the archive flag that turns it into `archived`.
    pub fn lifecycle(&self) -> PlanLifecycle {
        match (self.finalized_at, self.outcome()) {
            (None, _) => PlanLifecycle::Planning,
            (_, PlanOutcome::Approved) => PlanLifecycle::Approved,
            (_, PlanOutcome::Completed) => PlanLifecycle::Completed,
            (_, PlanOutcome::Abandoned) => PlanLifecycle::Abandoned,
        }
    }

    /// Closed outcomes refuse new work tags until reopened — the
    /// check both the summon tag and `setPlan` apply.
    pub fn terminal(&self) -> bool {
        self.finalized_at.is_some() && self.outcome().terminal()
    }

    /// The "all work finished" hint the panel shows beside its check-off
    /// action: the plan has something to finish — at least one item or
    /// linked employee — and every item is done or dropped and every
    /// linked employee expired. Never a state change.
    pub fn all_work_finished(&self, employees: &[BossEmployee]) -> bool {
        let mut linked = employees
            .iter()
            .filter(|entry| entry.plan_id == Some(self.id));
        let any_work = !self.items.is_empty() || linked.clone().next().is_some();
        any_work
            && self
                .items
                .iter()
                .all(|item| item.state != PlanItemState::ToDo)
            && linked.all(|entry| entry.expired)
    }
}

impl PlanItem {
    /// The item's render state — stored state, or `InProgress` while at
    /// least one employee linked to it is still live.
    pub fn progress(&self, plan: &BossPlan, employees: &[BossEmployee]) -> PlanItemProgress {
        match self.state {
            PlanItemState::Done => PlanItemProgress::Done,
            PlanItemState::Dropped => PlanItemProgress::Dropped,
            PlanItemState::ToDo => {
                let live = employees.iter().any(|entry| {
                    entry.plan_id == Some(plan.id)
                        && entry.item_id == Some(self.id)
                        && !entry.expired
                });
                if live {
                    PlanItemProgress::InProgress
                } else {
                    PlanItemProgress::ToDo
                }
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossState {
    pub identity: BossIdentity,
    pub persona_id: Uuid,
    /// The canonical Employee default persona — the stable identity of
    /// the shipped Employee role regardless of renames; a custom persona
    /// named "Employee" is never it. `None` while the human's choice
    /// between ambiguous legacy candidates is still open — employees
    /// cannot dispatch or resume until it is recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_persona_id: Option<Uuid>,
    /// The canonical identities of the shipped specialist employee
    /// roles — like `employee_persona_id`, the id names the record
    /// regardless of renames, so a custom persona sharing a role's name
    /// is never its default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub specialist_persona_ids: BTreeMap<PersonaDefaultRole, Uuid>,
    /// Revision provenance for the shipped default personas —
    /// starting/reviewed/seen revisions, recoverable text, and any open
    /// proposal.
    #[serde(default)]
    pub persona_defaults: PersonaDefaultsState,
    /// The consolidated persona-defaults upgrade notice awaiting delivery
    /// into the boss's next natural turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_default_notice: Option<PersonaDefaultNotice>,
    pub session_id: Option<Uuid>,
    pub personas: Vec<BossPersona>,
    pub employees: Vec<BossEmployee>,
    /// Retired identities remain available for revival until their name is
    /// assigned to another employee. They are not part of the visible roster.
    #[serde(default)]
    pub retired_employees: Vec<BossEmployee>,
    /// Documents saved before the rename record these under `bundles`.
    #[serde(default, alias = "bundles")]
    pub deliverables: Vec<BossDeliverable>,
    /// Open planning sessions and their plan documents. Records stay after
    /// finalization and archive — the freeze they carry is permanent.
    #[serde(default)]
    pub planning: Vec<BossPlan>,
    /// Daemon-owned outcomes — the durable records assignments serve,
    /// including their handoffs and finishing designations.
    #[serde(default)]
    pub outcomes: Vec<BossOutcome>,
    /// The daemon clock when the user last had the Goals page open —
    /// a goal finished since then reads as unread in the sidebar, the
    /// same contract `BossDeliverable::viewed_at` gives its row.
    #[serde(default)]
    pub goals_viewed_at: Option<u64>,
    /// The boss-set admission policy — provider+model caps and the
    /// desired host broker policy. Missing from older documents: no model
    /// caps until the boss sets rules.
    #[serde(default)]
    pub resource_policy: BossResourcePolicy,
    /// Next admission sequence number (1-based) — monotonic across
    /// restarts so a queued ticket's FIFO position survives.
    #[serde(default)]
    pub next_sequence: u64,
    /// Next dispatch-notification event id (1-based).
    #[serde(default)]
    pub next_event_id: u64,
    /// Position in the shuffled employee-name pool, retained across restarts.
    /// The pool is reshuffled when the daemon starts.
    #[serde(default)]
    pub name_cursor: u64,
    /// Durable dispatch notifications awaiting delivery to supervisors —
    /// drained by id so restart can neither drop nor duplicate one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outbox: Vec<DispatchNotification>,
    /// Wave membership and resolution — one record per `groupId` this
    /// boss's summons have used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waves: Vec<BossWave>,
    /// Durable wave-resolution notifications awaiting delivery — drained
    /// by id beside the dispatch outbox.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wave_outbox: Vec<WaveNotification>,
    pub revision: u64,
}

/// One work item inside a [`PlanGroup`] — its derived progress plus the
/// employees linked to it, in roster order.
pub struct PlanGroupItem<'a> {
    pub item: &'a PlanItem,
    pub progress: PlanItemProgress,
    pub employees: Vec<&'a BossEmployee>,
}

/// The Goals panel's ongoing-area group for one plan — the work
/// breakdown in declared order with each item's linked employees nested
/// beneath it, then the employees tagged to the plan but no live item.
/// Borrowed from the [`BossState`] snapshot the panel already holds.
pub struct PlanGroup<'a> {
    pub plan: &'a BossPlan,
    pub items: Vec<PlanGroupItem<'a>>,
    /// Employees tagged to the plan without a resolvable item — no tag,
    /// or a tag pointing at an item the breakdown no longer carries.
    pub unallocated: Vec<&'a BossEmployee>,
    /// The newest activity the group owns — a linked employee's summon
    /// or finish, the plan's freeze, or its latest audited transition.
    /// Groups order by it, most recent first.
    pub last_activity_at: u64,
}

impl BossState {
    /// The canonical persona record a default role names — the one that
    /// carries shipped-default provenance and review, regardless of its
    /// saved name.
    pub fn default_persona_id(&self, role: PersonaDefaultRole) -> Option<Uuid> {
        match role {
            PersonaDefaultRole::Boss => Some(self.persona_id),
            PersonaDefaultRole::Employee => self.employee_persona_id,
            _ => self.specialist_persona_ids.get(&role).copied(),
        }
    }

    /// The canonical role a saved persona carries, if it is one — a
    /// custom persona can share a role's name but never its identity.
    pub fn persona_default_role(&self, persona_id: Uuid) -> Option<PersonaDefaultRole> {
        if persona_id == self.persona_id {
            Some(PersonaDefaultRole::Boss)
        } else if self.employee_persona_id == Some(persona_id) {
            Some(PersonaDefaultRole::Employee)
        } else {
            self.specialist_persona_ids
                .iter()
                .find(|(_, id)| **id == persona_id)
                .map(|(role, _)| *role)
        }
    }

    /// Plan groups for the Goals ongoing area: every open plan that is
    /// approved or carries at least one tagged employee — a draft plan
    /// earns its row through work alone — ordered by most recent child
    /// activity. Completed and abandoned plans list in the Finished
    /// section instead. Employees within a bucket keep roster order;
    /// the panel owns any finer sorting.
    pub fn plan_groups(&self) -> Vec<PlanGroup<'_>> {
        let mut groups = self
            .planning
            .iter()
            .filter(|plan| !plan.terminal())
            .filter_map(|plan| {
                let linked = |item: Option<Uuid>| {
                    self.employees
                        .iter()
                        .filter(|entry| entry.plan_id == Some(plan.id) && entry.item_id == item)
                        .collect::<Vec<_>>()
                };
                let tagged = self
                    .employees
                    .iter()
                    .filter(|entry| entry.plan_id == Some(plan.id))
                    .count();
                if plan.lifecycle() == PlanLifecycle::Planning && tagged == 0 {
                    return None;
                }
                let items = plan
                    .items
                    .iter()
                    .map(|item| PlanGroupItem {
                        item,
                        progress: item.progress(plan, &self.employees),
                        employees: linked(Some(item.id)),
                    })
                    .collect::<Vec<_>>();
                let unallocated = self
                    .employees
                    .iter()
                    .filter(|entry| {
                        entry.plan_id == Some(plan.id)
                            && entry
                                .item_id
                                .is_none_or(|item| plan.items.iter().all(|known| known.id != item))
                    })
                    .collect::<Vec<_>>();
                let employee_activity = self
                    .employees
                    .iter()
                    .filter(|entry| entry.plan_id == Some(plan.id))
                    .filter_map(|entry| entry.expired_at.or(entry.created_at))
                    .max()
                    .unwrap_or(0);
                let last_activity_at = employee_activity
                    .max(plan.finalized_at.unwrap_or(0))
                    .max(plan.history.last().map(|entry| entry.at).unwrap_or(0));
                Some(PlanGroup {
                    plan,
                    items,
                    unallocated,
                    last_activity_at,
                })
            })
            .collect::<Vec<_>>();
        groups.sort_by(|a, b| b.last_activity_at.cmp(&a.last_activity_at));
        groups
    }
}

/// A task's stored lifecycle — set by an explicit audited act (a
/// finishing assignment's accepted success or an owner's `setOutcomeState`),
/// never derived from assignments finishing. `Open` doubles as the reopen target.
/// `NeedsAttention` is deliberately absent: attention derives from live
/// assignment and handoff state, so it can never disagree with it.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OutcomeState {
    #[default]
    Open,
    Completed,
    Cancelled,
}

impl OutcomeState {
    pub fn terminal(&self) -> bool {
        !matches!(self, OutcomeState::Open)
    }

    pub fn label(&self) -> &'static str {
        match self {
            OutcomeState::Open => "open",
            OutcomeState::Completed => "completed",
            OutcomeState::Cancelled => "cancelled",
        }
    }
}

/// One entry in a task's audit trail — the outcome a transition entered,
/// when, and who moved it. A `Completed`/`Cancelled` followed by `Open`
/// is a reopen.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeTransition {
    pub state: OutcomeState,
    pub at: u64,
    pub actor: PlanActor,
}

/// The boss's decision on a pending handoff, as `resolveHandoff` takes
/// it. `assign` references the follow-up assignment the boss already
/// summoned; `completeOutcome` accepts the assignment's result as the outcome
/// and records `evidence` — an explicit completion that notifies nobody.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HandoffDecision {
    /// Follow-up work was assigned — the new assignment's employee session id.
    Assign { assignment: Uuid },
    /// The result needed no follow-up.
    Dismiss,
    /// The result already achieved the outcome — complete the task with
    /// this evidence.
    CompleteOutcome { evidence: String },
}

/// A settled handoff — the boss's decision and when it landed.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HandoffResolution {
    pub decision: HandoffDecision,
    pub at: u64,
}

/// A successful ordinary assignment's durable claim on the boss: the result
/// plus the follow-up intent captured at assignment. It survives boss
/// session rotation — the record is daemon-owned, and a rotated session
/// sees it through `view` and the work digest. Resolution is explicit;
/// reading the delivered report alone never settles one.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeHandoff {
    pub id: Uuid,
    /// The assignment's employee session id.
    pub assignment: Uuid,
    /// The ticket generation whose success produced it — the attempt
    /// identity, so a re-driven finish cannot write a second handoff.
    #[serde(default)]
    pub attempt: u64,
    /// The follow-up intent captured at assignment — `Assignment`'s
    /// `after_success` verbatim.
    #[serde(default)]
    pub intent: String,
    pub created_at: u64,
    /// `None` while the boss's decision is still owed — a pending
    /// handoff counts as unresolved task work and blocks completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolution: Option<HandoffResolution>,
}

impl OutcomeHandoff {
    pub fn pending(&self) -> bool {
        self.resolution.is_none()
    }
}

/// A finishing assignment's success that the completion conditions
/// refused — the durable issue the task's attention state reports until
/// the boss resolves it. The assignment's result is retained; the task is
/// unfinished.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CompletionConflict {
    /// The finishing assignment's employee session id.
    pub assignment: Uuid,
    /// The ticket generation whose success conflicted.
    #[serde(default)]
    pub attempt: u64,
    /// The specific outstanding items at acceptance — unfinished assignments,
    /// unresolved failures, or pending handoffs.
    pub reason: String,
    pub at: u64,
}

/// The panel-facing task status — the stored outcome plus derived
/// attention. Never persisted; it recomputes from live assignment state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutcomeStatus {
    Open,
    NeedsAttention,
    Completed,
    Cancelled,
}

/// The coarse verdict a settled assignment attempt carries — frozen at
/// settle time so the row still classifies after the roster record is
/// gone. The finer-grained [`ExpiryCause`] rides beside it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AssignmentVerdict {
    /// The admission ended cleanly. Records too old to carry a cause
    /// read as finished rather than as phantom failures, matching
    /// [`BossOutcome::assignment_succeeded`].
    Finished,
    /// The admission failed or interrupted — a failed turn or launch, a
    /// provider exit mid-turn, a restart, or a settle that left parked
    /// prompts or an unanswered ask. A flagged blocker lands here too:
    /// the job could not finish on its own.
    Failed,
    /// A supervisor stopped the admission.
    Cancelled,
    /// A live requeue replaced the admission before it settled — a
    /// model/provider reconfiguration relaunches the same job under a
    /// new generation rather than settling this one.
    Superseded,
    /// The attempt ended before durable history recorded it — only a
    /// reference or a generation number survived, never a result. A
    /// row reads this as "details unavailable", never as success.
    Unavailable,
}

impl AssignmentVerdict {
    /// The wire label — digests and lists render the verdict compactly.
    pub fn label(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Superseded => "superseded",
            Self::Unavailable => "unavailable",
        }
    }
}

/// How one settled assignment attempt ended — the roster record's verdict
/// frozen at expiry so the row still answers after retirement, name
/// reuse, or restart removes the employee.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentSettle {
    pub verdict: AssignmentVerdict,
    /// The settle classification the expired record carried — `None` on
    /// records that predate `expiry` and on rows recovered from a
    /// reference that proves the verdict without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cause: Option<ExpiryCause>,
    /// The employee flagged a blocker before it settled.
    #[serde(default)]
    pub blocked: bool,
    /// When the admission settled — `None` where no durable record
    /// carried the time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub at: Option<u64>,
}

/// One admission's durable record on an outcome — appended when the
/// assignment links the employee to the outcome and resolved when the
/// admission settles. The outcome owns the list, so employee retirement,
/// name reuse, and daemon restart cannot drop the history, and every
/// re-admission — resume, retry, requeue — lands as a distinct row under
/// a fresh generation rather than rewriting the settled one.
///
/// `session` doubles as the conversation reference: the task transcript
/// is keyed by session id and survives the roster record under the
/// ordinary retention policy, so the row references the conversation
/// rather than copying it.
///
/// Rows written before this record existed are recovered at load where
/// durable evidence survives: a roster record fills every field, while a
/// dangling handoff, finisher, or prerequisite reference yields a sparse
/// row whose missing fields stay `None` — unavailable, never invented.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeAssignment {
    /// The assignment's employee session id — its identity and
    /// conversation reference in one.
    pub session: Uuid,
    /// The admission generation the attempt ran — 1+ for ticketed
    /// admissions; 0 where the admission predates tickets or its
    /// generation is unrecoverable.
    #[serde(default)]
    pub generation: u64,
    /// The assignee's identity frozen at admission — `None` on rows
    /// whose roster record was already gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub identity: Option<BossIdentity>,
    /// The job title captured at admission — `None` where it could not
    /// be recovered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub job_title: Option<String>,
    /// Whether this was the designated finishing assignment — `None`
    /// where the completion behavior could not be recovered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub finishes_outcome: Option<bool>,
    /// The follow-up intent captured at admission — `None` for
    /// finishers, which carry none, and where it is unrecoverable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after_success: Option<String>,
    /// The sibling session ids this admission waited on — kept so a
    /// waiting row can still name its dependency after the
    /// prerequisite's record is gone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<Uuid>,
    /// When the admission entered — `None` where no surviving record
    /// carried the time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub assigned_at: Option<u64>,
    /// The settle verdict — `None` while the admission is still in
    /// flight; a recovered row whose outcome is lost carries
    /// [`AssignmentVerdict::Unavailable`] rather than staying open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub settled: Option<AssignmentSettle>,
}

/// A daemon-owned outcome: the durable record assignments serve. One
/// level of nesting — assignments are employees linked by
/// `Assignment::outcome_id`; at most one of them is the designated finishing
/// assignment at a time. Nothing about the record notifies the boss: a
/// finishing success updates it silently, while failures, blockers, and
/// completion conflicts surface through the assignment's ordinary report
/// path.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossOutcome {
    pub id: Uuid,
    /// What the assignments should achieve — the row's outcome text.
    pub outcome: String,
    /// What "done" means — a `finishesOutcome` summon refuses an
    /// outcome whose criteria are not explicit.
    #[serde(default)]
    pub success_criteria: String,
    /// The stored lifecycle — attention and per-assignment states stay
    /// derived. Set only through `setOutcomeState` or an accepted
    /// finishing success.
    #[serde(default)]
    pub state: OutcomeState,
    /// The designated finishing assignment's employee session id — the only
    /// assignment whose success may complete the task. A cancelled finisher
    /// clears it; an expired one's replacement overwrites it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub finishing_assignment: Option<Uuid>,
    /// Every ordinary-assignment success handoff, in creation order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handoffs: Vec<OutcomeHandoff>,
    /// The unresolved closure conflict, when a closing success could not
    /// close. Cleared by an accepted close, an explicit outcome change,
    /// or a replacement closer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub completion_conflict: Option<CompletionConflict>,
    /// The evidence recorded at completion — the finishing assignment's
    /// transcript index or the owner's completion note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub evidence: Option<String>,
    /// The approved plan attached to this task — a `BossPlan::id` set by
    /// `attachPlan` or a `newOutcome` summon carrying `plan`. The plan
    /// describes the approach; the task alone owns execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plan_id: Option<Uuid>,
    /// A tracked wait the boss recorded — a date or dependency that
    /// explains the pause. Its presence suppresses unattended reminders
    /// until it clears.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub waiting: Option<OutcomeWait>,
    /// Reminders for this task sleep until this timestamp — a snooze
    /// with an expiry, never an indefinite untracked promise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub snoozed_until: Option<u64>,
    /// The last meaningful activity: assignment, result, or a recorded
    /// decision. The unattended grace clock starts here — merely viewing
    /// the task does not move it.
    #[serde(default)]
    pub last_activity_at: u64,
    /// When the task first entered reminder eligibility — open, idle,
    /// and with no pending handoff or issue covering it. The reminder
    /// scan maintains it: `None` whenever eligibility conditions fail,
    /// so a new eligible stretch starts a fresh grace period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unattended_since: Option<u64>,
    /// The last delivered reminder for the current unattended period —
    /// which period it covered and when it went out, so repeats hold to
    /// the daily cadence and a reopened period reminds afresh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_reminder: Option<OutcomeReminder>,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub completed_at: Option<u64>,
    /// Audited outcome transitions — closes, cancels, and reopens in
    /// order, each with actor and time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<OutcomeTransition>,
    /// Durable assignment history — one row per admission attempt, in
    /// admission order. The outcome owns the list so the history reads
    /// the same after the employees' roster records retire or the
    /// daemon restarts; rows recovered from older data carry only what
    /// survived.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<OutcomeAssignment>,
}

/// A tracked pause on a task — why nothing is assigned right now. A
/// reminder skips a waiting task until the wait clears: a `until` wait
/// clears at its timestamp, a `dependency` wait clears when the boss
/// clears it or the named work resolves.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum OutcomeWait {
    /// Waiting until a point in time — a deliberate deferral, not an
    /// unattended task.
    Until { at: u64 },
    /// Waiting on a tracked dependency described in `note` — another
    /// task or external event the boss can point at.
    Dependency { note: String },
}

/// One delivered unattended-task reminder: which unattended period it
/// covered (`unattended_since` value at send time) and when it went out.
/// A period repeats at most once per day; a fresh period re-arms the
/// first-reminder path.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeReminder {
    pub unattended_since: u64,
    pub at: u64,
}

/// The daemon clock's grace and repeat cadence for unattended-task
/// reminders — fifteen minutes of quiet before the first batched
/// reminder, then at most one per day per uninterrupted period.
pub const OUTCOME_UNATTENDED_GRACE: u64 = 15 * 60;
pub const OUTCOME_REMINDER_INTERVAL: u64 = 24 * 60 * 60;

/// The `newOutcome` summon input — the parent outcome and success criteria
/// created atomically with the assignment's first assignment.
#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NewOutcome {
    /// The outcome the assignments work toward — the row's title.
    pub outcome: String,
    /// What "done" means — required up front so a `finishesOutcome`
    /// assignment can be assigned into the task without a second step.
    #[serde(default)]
    pub success_criteria: String,
}

impl BossOutcome {
    /// Closed outcomes refuse new work until reopened.
    pub fn terminal(&self) -> bool {
        self.state.terminal()
    }

    /// Handoffs still owed a boss decision — they count as unresolved
    /// goal work and block closure.
    pub fn pending_handoffs(&self) -> impl Iterator<Item = &OutcomeHandoff> {
        self.handoffs.iter().filter(|handoff| handoff.pending())
    }

    /// The assignment's outcome for completion purposes: an accepted success is
    /// an expired record with a clean finish and no flagged blocker;
    /// cancelled assignments count for nothing.
    pub fn assignment_succeeded(employee: &BossEmployee) -> bool {
        employee.expired
            && !employee.cancelled
            && employee.blocker.is_none()
            && employee
                .expiry
                .as_ref()
                .map(|expiry| expiry.cause == ExpiryCause::Finished)
                // Records predating `expiry` carry no settle — an expired
                // row reads as finished rather than as a phantom failure.
                .unwrap_or(true)
    }

    /// An unresolved failure or intervention blocker on this assignment —
    /// the task's needs-attention source besides a completion conflict.
    /// A cancelled assignment's issue is resolved by the cancel itself; a
    /// retried or resumed one returns to active work when re-admission
    /// clears its expiry and blocker.
    pub fn assignment_unresolved(employee: &BossEmployee) -> bool {
        if employee.cancelled {
            return false;
        }
        employee.blocker.is_some()
            || (employee.expired
                && employee
                    .expiry
                    .as_ref()
                    .is_some_and(|expiry| expiry.cause.reports()))
    }

    /// Whether an unresolved failure, intervention blocker, or closure
    /// conflict exists — the derived half of [`OutcomeStatus`]. Pending
    /// handoffs are "awaiting follow-up" — Open, not attention.
    pub fn needs_attention(&self, employees: &[BossEmployee]) -> bool {
        self.completion_conflict.is_some()
            || employees.iter().any(|employee| {
                employee
                    .assignment
                    .as_ref()
                    .is_some_and(|assignment| assignment.outcome_id == self.id)
                    && Self::assignment_unresolved(employee)
            })
    }

    /// The renderable status — stored outcome, or attention over open.
    pub fn status(&self, employees: &[BossEmployee]) -> OutcomeStatus {
        match self.state {
            OutcomeState::Completed => OutcomeStatus::Completed,
            OutcomeState::Cancelled => OutcomeStatus::Cancelled,
            OutcomeState::Open if self.needs_attention(employees) => OutcomeStatus::NeedsAttention,
            OutcomeState::Open => OutcomeStatus::Open,
        }
    }

    /// Whether the recorded wait or snooze still suppresses reminders at
    /// `now` — an expired `until` wait or elapsed snooze stops shielding
    /// the task so a forgotten deferral cannot hide it forever.
    pub fn wait_shields(&self, now: u64) -> bool {
        if self.snoozed_until.is_some_and(|until| until > now) {
            return true;
        }
        match &self.waiting {
            Some(OutcomeWait::Until { at }) => *at > now,
            Some(OutcomeWait::Dependency { .. }) => true,
            None => false,
        }
    }

    /// A assignment this task's reminders treat as live work — not expired
    /// and not cancelled. Queued, dispatching, and working all count:
    /// capacity waiting is already-visible Waiting work.
    fn live_assignment(employee: &BossEmployee) -> bool {
        !employee.expired && !employee.cancelled
    }

    /// Whether the task currently qualifies for an unattended reminder:
    /// open, no live assignment, no pending handoff, no intervention issue —
    /// and not shielded by a recorded wait or snooze. A task covered by
    /// its own pending delivery is never "missing coordination."
    pub fn reminder_eligible(&self, employees: &[BossEmployee], now: u64) -> bool {
        !self.terminal()
            && !self.wait_shields(now)
            && self.completion_conflict.is_none()
            && self.pending_handoffs().next().is_none()
            && !employees.iter().any(|employee| {
                employee
                    .assignment
                    .as_ref()
                    .is_some_and(|assignment| assignment.outcome_id == self.id)
                    && (Self::live_assignment(employee) || Self::assignment_unresolved(employee))
            })
    }

    /// When the current unattended stretch became reminder-eligible —
    /// the grace clock's start. `unattended_since` marks entry into
    /// eligibility; `last_activity_at` marks the last meaningful event,
    /// and the reminder waits past both.
    fn grace_started(&self) -> Option<u64> {
        self.unattended_since
            .map(|since| since.max(self.last_activity_at))
    }

    /// Whether a reminder should deliver at `now` — the grace period has
    /// lapsed and either this unattended period has never reminded or the
    /// daily interval has elapsed since its last delivery.
    pub fn reminder_due(&self, now: u64) -> bool {
        let Some(start) = self.grace_started() else {
            return false;
        };
        if now < start + OUTCOME_UNATTENDED_GRACE {
            return false;
        }
        match &self.last_reminder {
            None => true,
            Some(reminder) => {
                reminder.unattended_since != self.unattended_since.unwrap_or(0)
                    || now >= reminder.at + OUTCOME_REMINDER_INTERVAL
            }
        }
    }

    /// The specific items standing between the outcome and accepting
    /// `closer`'s success — unfinished non-cancelled assignments, unresolved
    /// failures, and pending handoffs, named for the conflict report or
    /// a waiting closer's detail. Closure also re-checks these, so a
    /// state that could not dispatch never settles as completed.
    pub fn completion_outstanding(
        &self,
        employees: &[BossEmployee],
        retired: &[BossEmployee],
        closer: Uuid,
    ) -> Vec<String> {
        let mut outstanding = Vec::new();
        for employee in employees.iter().chain(retired.iter()) {
            let belongs = employee
                .assignment
                .as_ref()
                .is_some_and(|assignment| assignment.outcome_id == self.id);
            if !belongs || employee.session_id == closer || employee.cancelled {
                continue;
            }
            if !employee.expired {
                outstanding.push(format!(
                    "assignment \"{}\" ({}) is still working",
                    employee.job_title, employee.identity.name
                ));
            } else if !Self::assignment_succeeded(employee) {
                outstanding.push(format!(
                    "assignment \"{}\" ({}) has an unresolved failure or blocker",
                    employee.job_title, employee.identity.name
                ));
            }
        }
        let pending = self.pending_handoffs().count();
        if pending > 0 {
            outstanding.push(format!(
                "{pending} handoff{} await{} a decision",
                if pending == 1 { "" } else { "s" },
                if pending == 1 { "s" } else { "" }
            ));
        }
        outstanding
    }
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
    rename_all_fields = "camelCase",
    deny_unknown_fields
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
    /// Boss files root; `title` is the idea the sidebar row displays.
    /// Boss-only; the user never creates one directly. Omitted
    /// provider/model/effort fields take the daemon's planning default
    /// (codex `gpt-6.1-sol` at medium effort) rather than inheriting the
    /// boss chat's own pick.
    CreatePlan {
        title: String,
        plan_file: String,
        prompt: String,
        #[serde(default)]
        provider: Option<ProviderKind>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        reasoning_effort: Option<String>,
    },
    /// Open a web page in the boss chat's right panel. Boss principals only.
    Browse {
        url: String,
        #[serde(default)]
        title: Option<String>,
    },
    /// Ask the connected desktop app to create a pinned, standalone terminal.
    /// Terminals belong to the app rather than the daemon; the daemon emits
    /// this intent on the principal's session stream to the client that
    /// prompted the turn — broadcasting to every client when no prompting
    /// client is recorded. Boss principals and the human may create
    /// terminals, employees may not.
    Terminal {
        title: String,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        command: Option<String>,
    },
    /// Freeze a plan document after user approval. A planning session
    /// finalizes its own plan (`plan_file` omitted); the boss chat or a
    /// human names the file. Approval lands on a daemon-owned request card;
    /// the approved design is handed to the boss chat for implementation,
    /// and the session archives once the grace period elapses. `items`
    /// seeds the work breakdown from the approved document's course of
    /// work — what the user signed off on is what the list starts as;
    /// omit it to leave an already-declared breakdown unchanged.
    FinalizePlan {
        #[serde(default)]
        plan_file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        items: Option<Vec<String>>,
    },
    /// Manage the user's daemon-owned scheduled automations. Boss-only.
    Automation {
        action: AutomationOperation,
    },
    Summon {
        /// The employee role layered on the canonical Employee base — a
        /// shipped specialist or a custom persona. `None` assigns the
        /// base alone; passing the canonical Employee persona is the same
        /// default-only assignment, and the canonical Boss persona is
        /// rejected.
        #[serde(default)]
        persona_id: Option<Uuid>,
        #[serde(alias = "name")]
        job_title: String,
        prompt: String,
        project: String,
        #[serde(default)]
        provider: Option<ProviderKind>,
        #[serde(default)]
        model: Option<String>,
        /// Optional effort pin for the employee's session, validated
        /// against the resolved model's catalog — an unsupported id fails
        /// the summon rather than silently applying another effort.
        #[serde(default)]
        reasoning_effort: Option<String>,
        /// Where the employee's checkout runs; `None` uses the project
        /// itself, `worktree` forks a daemon-managed Git worktree, and
        /// `adopt` hands it a finished employee's worktree (see
        /// `adopt_worktree`).
        #[serde(default)]
        workspace: Option<AgentWorkspace>,
        /// The ref a worktree summon starts from; required when
        /// `workspace` is `worktree`, ignored otherwise — an `adopt`
        /// summon keeps whatever the worktree already contains.
        #[serde(default)]
        base_branch: Option<String>,
        /// The daemon-managed worktree a `workspace: "adopt"` summon
        /// takes over: a registered worktree of the project repo whose
        /// owning ticket is finished. Required for `adopt`, ignored
        /// otherwise.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        #[ts(type = "string")]
        adopt_worktree: Option<PathBuf>,
        /// Per-field grant overrides persisted on the employee record;
        /// `None` inherits the persona's permissions unchanged.
        #[serde(default)]
        permissions: Option<PermissionOverrides>,
        /// The work kind fixed at summon — `errand` (the default) reports
        /// its finish to the supervisor; `goal` expires silently and lists
        /// on the client's Goals page.
        #[serde(default)]
        work_goal: EmployeeGoal,
        /// Icon for this employee — overrides the persona's icon for this
        /// summon only. `None` inherits the persona default; when neither
        /// names one the client derives the icon from the job title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        icon: Option<crate::custom_commands::CustomCommandIcon>,
        /// Host resources the assignment reserves for its lifetime — the
        /// employee's own `resource run` calls borrow subsets of the
        /// granted set rather than re-queuing. Empty means the job claims
        /// no host resources. A set larger than total host capacity fails
        /// the summon at submission.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        resources: Option<crate::resources::ResourceSet>,
        /// Authorize this summon to spend burst slots above the model
        /// rule's `liveLimit` — it still cannot pass `hardCap`. Without
        /// it the ticket waits at `liveLimit`.
        #[serde(default)]
        allow_burst: bool,
        /// Group the summons into a wave: every ticket admitted under
        /// the same id is a member, and the daemon reports once to the
        /// supervisor when all members reach a terminal state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        group_id: Option<String>,
        /// Scheduling hint seam — stored on the admission record; the
        /// strict-FIFO scheduler does not reorder on it yet.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        priority: Option<i64>,
        /// Link the assignment to a daemon-owned [`BossOutcome`] as one of
        /// its assignments — an unknown or closed task fails the summon.
        /// Ordinary assignment finishes leave a durable handoff on the task;
        /// `finishes_outcome` and `after_success` fix the completion
        /// behavior. Mutually exclusive with `new_outcome`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        outcome_id: Option<Uuid>,
        /// Create the parent task in the same admission — the outcome
        /// and success criteria the first assignment serves. The creation
        /// is atomic with this assignment: a failed summon leaves no
        /// orphan task behind. Mutually exclusive with `outcome_id`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        new_outcome: Option<NewOutcome>,
        /// What the boss should decide or do with this assignment's result —
        /// captured now, delivered with the finish report, and kept on
        /// the durable handoff. Ordinary assignments only: a `finishes_outcome`
        /// summon carrying intent is rejected, and an omitted intent
        /// stores [`DEFAULT_AFTER_SUCCESS`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        after_success: Option<String>,
        /// Designate this assignment the task's finisher — on success it
        /// completes the task without notifying the boss instead of
        /// leaving a handoff. Requires `outcome_id` or `new_outcome`, explicit
        /// success criteria on the task, and no other live finisher.
        #[serde(default)]
        finishes_outcome: bool,
        /// Sibling assignments (employee session ids on the same task) whose
        /// accepted success must land before this assignment dispatches.
        /// A fresh `new_outcome` has no siblings to name — combine it with
        /// an existing `outcome_id` only.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        prerequisites: Vec<Uuid>,
        /// Tag the assignment to a plan — the `BossPlan::id`, its
        /// planning-session id, or its `plans/<file>.md` path. An unknown
        /// plan or a plan with a closed outcome fails the summon rather
        /// than landing untagged; a still-open draft tags fine. The tag
        /// is the durable, user-visible grouping — orthogonal to
        /// `work_goal`, `outcome_id`, and `group_id`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        plan: Option<String>,
        /// The `PlanItem::id` inside `plan` the assignment serves —
        /// requires `plan`; an unknown or already done/dropped item
        /// fails the summon. Omitted lists the employee under the plan's
        /// unallocated work.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        item: Option<Uuid>,
        /// Idempotency key: a retry that lost its response returns the
        /// original employee rather than a duplicate. Reusing the id with
        /// different summon fields is an error.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        request_id: Option<Uuid>,
    },
    /// Replace the boss-set admission policy atomically. `expectedRevision`
    /// must match the current `resource_policy.revision`. Boss/human only —
    /// employees cannot raise caps. Lowering a cap never kills running
    /// work; it blocks new admissions until usage drains.
    SetResourcePolicy {
        expected_revision: u64,
        model_limits: Vec<ModelLimit>,
        /// Optional host section — updates the resource broker's policy
        /// file under its authority lock in the same accepted update.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        host: Option<crate::resources::ResourcePolicy>,
    },
    /// Reshape a plan's ordered work breakdown in one call: an entry
    /// naming an existing item `id` renames and repositions it, an entry
    /// without one appends a new `toDo` item, and an item the list omits
    /// is marked `dropped` — the declared list is the whole course, and
    /// omissions drop (audited, struck, reopenable) rather than delete.
    /// Usable before approval — a planning session declares its course
    /// early — and after; a closed plan refuses until reopened.
    /// Boss/human only.
    UpdatePlanItems {
        /// The `BossPlan::id`, its planning-session id, or its
        /// `plans/<file>.md` path.
        plan: String,
        items: Vec<PlanItemInput>,
    },
    /// Bookkeep one work item: mark it `done` or `dropped`, or return it
    /// to `toDo` on reopen. `inProgress` is derived and never set.
    /// Boss/human only.
    SetPlanItemState {
        plan: String,
        /// The `PlanItem::id` to update.
        item: Uuid,
        state: PlanItemState,
    },
    /// Set a finalized plan's outcome: `completed` or `abandoned` closes
    /// it, `approved` reopens a closed plan. Every transition lands on the
    /// record's audit trail with the caller's actor and time. A draft has
    /// no outcome to set. Boss/human only.
    SetPlanOutcome {
        plan: String,
        outcome: PlanOutcome,
    },
    /// Create a daemon-owned outcome assignments can be assigned to via
    /// summon's `outcomeId`. The outcome opens with no assignments — it
    /// completes only through an accepted finishing assignment or an
    /// explicit `setOutcomeState`, never from an empty or finished
    /// assignment list. Boss/human only.
    CreateOutcome {
        /// The outcome the assignments work toward — the row's title.
        outcome: String,
        /// What "done" means — a `finishesOutcome` summon refuses the task
        /// while this is empty.
        #[serde(default)]
        success_criteria: String,
    },
    /// Move an outcome between `open`, `completed`, and `cancelled`:
    /// `completed` requires `evidence` and no pending handoffs,
    /// `cancelled` abandons the outcome and stops its live assignments,
    /// and `open` reopens a closed outcome — a cancelled outcome's late
    /// results cannot revive it. Every transition lands on the audit
    /// trail and notifies nobody. Boss/human only.
    SetOutcomeState {
        /// The `BossOutcome::id`.
        outcome: Uuid,
        state: OutcomeState,
        /// Required for `completed` — the recorded result evidence;
        /// optional for `cancelled` and `open` as the reason note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        evidence: Option<String>,
    },
    /// Resolve one pending handoff on an outcome — the boss's explicit
    /// decision on the assignment's result. `completeOutcome` completes
    /// the outcome silently with the given evidence; the other pending
    /// handoffs, if any, still block it. Boss/human only.
    ResolveHandoff {
        /// The `BossOutcome::id`.
        outcome: Uuid,
        /// The `OutcomeHandoff::id` to settle.
        handoff: Uuid,
        decision: HandoffDecision,
    },
    /// Record a tracked wait or snooze on an outcome — how the boss
    /// explains a deliberate pause so unattended reminders leave it
    /// alone. A `null` clears the field. Boss/human only.
    SetOutcomeWaiting {
        /// The `BossOutcome::id`.
        outcome: Uuid,
        /// `until` waits suppress reminders until the timestamp; a
        /// `dependency` wait suppresses them until cleared.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        waiting: Option<OutcomeWait>,
        /// Sleep reminders until this timestamp — `null` clears.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        snoozed_until: Option<u64>,
    },
    /// Attach an approved plan to an outcome — the approach record the
    /// assignments execute against. Attaching supersedes any previous
    /// attachment; it never changes outcome state or starts work.
    /// Boss/human only.
    AttachPlan {
        /// The `BossOutcome::id`.
        outcome: Uuid,
        /// The `BossPlan::id`, its planning-session id, or its
        /// `plans/<file>.md` path to attach.
        plan: String,
    },
    /// Toggle `merge submit` landings for a registered project — off by
    /// default, so the boss opts a project in before its employees can
    /// submit. `project` is a registered project's name, id, or root path.
    /// Boss/human only.
    SetProjectSubmissions {
        project: String,
        enabled: bool,
    },
    /// Retarget a registered project's QA branch — the branch `merge
    /// submit` landings rebase onto and the review train reads. `branch`
    /// must be a usable branch name; omit it or pass null to clear the
    /// override and re-inherit the daemon-global `qa_branch` setting.
    /// `project` is a registered project's name, id, or root path.
    /// Boss/human only.
    SetProjectQaBranch {
        project: String,
        #[serde(default)]
        branch: Option<String>,
    },
    Control {
        session_id: Uuid,
        action: EmployeeControl,
    },
    /// Revive an expired employee in place — re-admit its ticket through
    /// the ordinary queue with a synthesized "verify and continue" prompt
    /// so the transcript, workspace, and provider cursor all survive.
    /// Same caller gate as `control`: the human, the boss, or the
    /// employee's supervisor. A record that is not expired refuses.
    Resume {
        session_id: Uuid,
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
    /// Change the avatar generator without re-rolling seeds. The style is
    /// global — it applies to the boss and every employee at once, so
    /// `session_id` is retained for compatibility and ignored. Human-only.
    SetAvatarStyle {
        session_id: Option<Uuid>,
        avatar_style: AvatarStyle,
    },
    UpsertPersona {
        persona: BossPersonaUpsert,
    },
    /// Inspect, reset, or review the shipped Boss/Employee default
    /// instructions — `action` picks the step. Read actions are
    /// boss/human; `keep` and `adopt` are human-only since customized
    /// instructions change only on human approval.
    PersonaDefault {
        action: PersonaDefaultAction,
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
    /// this daemon's host; `name` defaults to the path's file name. The
    /// daemon copies the target into its deliverable store so the sidebar
    /// entry survives the workspace it came from — `reference: true` opts
    /// out and keeps a live filesystem path, for artifacts too large to copy
    /// or meant to stay current.
    PublishDeliverable {
        path: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        reference: bool,
    },
    DismissDeliverable {
        id: Uuid,
    },
    /// Direct deterministic access to the Boss's file-canonical memory store.
    Memory {
        operation: MemoryOperation,
    },
    /// Pin or unpin a deliverable's sidebar row; pinned deliverables lead the group
    /// and never age out of it.
    PinDeliverable {
        id: Uuid,
        pinned: bool,
    },
    /// Sweep a deliverable behind the group's dormant fold, or restore it to the
    /// live list.
    SweepDeliverable {
        id: Uuid,
        dormant: bool,
    },
    /// Archive or unarchive a deliverable — archived deliverables leave the sidebar
    /// but keep their record.
    ArchiveDeliverable {
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
    /// Stamp the deliverable viewed at the daemon's clock — opening its page or
    /// preview retires the sidebar's unread marker.
    MarkDeliverableViewed {
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
    rename_all_fields = "camelCase",
    deny_unknown_fields
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
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MemoryOperation {
    ListBuckets,
    CreateBucket {
        name: String,
        #[serde(default)]
        purpose: String,
    },
    /// `bucket` is an explicit bucket id; `project` names a registered
    /// project or an absolute project root and resolves to that project's
    /// shared bucket. When both are absent, an employee caller's assigned
    /// project bucket is used. `bucket` and `project` are mutually
    /// exclusive.
    Overview {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bucket: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
    },
    Record {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bucket: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        kind: MemoryNoteKind,
        text: String,
        retry_key: String,
    },
    SubmitSummary {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bucket: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        start: u64,
        end: u64,
        text: String,
    },
    Scan {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bucket: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        query: String,
    },
    ZoomBucket {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bucket: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        start: u64,
        end: u64,
    },
    /// Inspect or append old memory files into an explicit named bucket.
    /// `source` is `boss` or an absolute project root. This operation is
    /// Boss-only; dry runs return every candidate without changing storage.
    MigrateLegacy {
        bucket: String,
        source: String,
        #[serde(default = "default_true")]
        dry_run: bool,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemoryNoteKind {
    Fact,
    Observation,
    Question,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMigrationCandidate {
    pub source: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMigrationReport {
    pub bucket: String,
    pub source: String,
    pub dry_run: bool,
    pub candidates: Vec<MemoryMigrationCandidate>,
    pub imported: usize,
}

fn default_true() -> bool {
    true
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
    /// The daemon accepted a terminal intent and emitted it to the app.
    TerminalRequested {
        title: String,
        cwd: String,
    },
    /// `Summoned` always means accepted — a `queued` state is a ticket
    /// that starts when capacity frees, never an error. `state` and
    /// `admission` default for results read by older clients.
    Summoned {
        session_id: Uuid,
        #[serde(default)]
        state: EmployeeLifecycle,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        admission: Option<SummonAdmission>,
    },
    /// The accepted resource policy — both effective policies after the
    /// atomic replace.
    ResourcePolicySet {
        policy: BossResourcePolicy,
    },
    Transcript {
        transcript: crate::model::AgentSessionTranscript,
    },
    /// The shipped-default inspection report — one record per canonical
    /// role with full texts, revision labels, provenance, and diffs.
    PersonaDefaults {
        defaults: Vec<PersonaDefaultInfo>,
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
        #[serde(default)]
        buckets: Vec<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        overview: Option<serde_json::Value>,
        #[serde(default)]
        notes: Vec<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        compression: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recorded: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bucket: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        migration: Option<MemoryMigrationReport>,
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
        /// How the prompt reaches a working employee: `interrupt`
        /// (default) steers into an open turn and parks behind one when
        /// there is none, `queue` always waits for the current work to
        /// settle, and `steer` requires a live turn. A queued or
        /// dispatching employee takes the prompt into its launch
        /// envelope either way — there is no turn to interrupt yet.
        #[serde(default)]
        #[ts(optional)]
        delivery: Option<crate::protocol::AgentPromptDelivery>,
    },
    Steer {
        prompt: String,
        /// Retitles the job when the steer redirects the assignment —
        /// bookkeeping on the roster record only: it queues no prompt,
        /// wakes nothing, and writes no transcript entry. Omitted leaves
        /// the title unchanged.
        #[serde(default)]
        #[ts(optional)]
        job_title: Option<String>,
    },
    /// Apply a catalog-listed provider/model selection as one atomic
    /// reconfigure: a turn in flight is interrupted — recorded as an
    /// intentional stop, not a failure — the new selection applies, and
    /// the employee resumes its assignment on it. A provider+model change
    /// re-enters admission against the new pool; a same-model change keeps
    /// the runtime and continues in place. Queued tickets reticket in
    /// place. An omitted `reasoning_effort` resolves inside the target
    /// model's ladder — a rung packed into the model id first, then the
    /// catalog default — never the stale effort a resumed provider thread
    /// would keep.
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
    /// Move the employee to a different workspace as one action: its
    /// current turn is interrupted, the session rebinds — `worktree` forks
    /// a fresh daemon-managed worktree off `base_branch`, `local` returns
    /// it to the project's primary checkout — and the same transcript
    /// resumes there. A failure anywhere leaves the employee running in
    /// its old workspace. `adopt` is not a move target — adopting another
    /// employee's worktree happens at summon.
    SetWorkspace {
        workspace: AgentWorkspace,
        /// The ref the new worktree detaches at; required when `workspace`
        /// is `worktree`, ignored for `local`.
        #[serde(default)]
        base_branch: Option<String>,
    },
    /// Replace the host-resource set the assignment holds. A queued
    /// ticket is rewritten in place and re-enters admission immediately —
    /// it may now wait on the declared set. A working employee is never
    /// stopped: the daemon retries admission for the new set under a
    /// parked reservation id and swaps it in atomically once granted,
    /// releasing the old claims — until then the employee keeps running
    /// on its current set. Setting the currently held set cancels a
    /// parked update.
    SetResources {
        resources: crate::resources::ResourceSet,
    },
    /// Re-tag the employee's plan and item links — the mutable half of
    /// the summon tag, so a mis-tagged employee moves groups without a
    /// resummon. Both fields are tri-state: an absent field keeps the
    /// current link, `null` clears it, and a value re-tags. Re-tagging
    /// the plan without restating `item` drops the employee to the plan's
    /// unallocated list; an `item` set resolves against the employee's
    /// effective plan and validates like the summon tag.
    SetPlan {
        #[serde(
            default,
            deserialize_with = "double_option::deserialize",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        plan: Option<Option<String>>,
        #[serde(
            default,
            deserialize_with = "double_option::deserialize",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        item: Option<Option<Uuid>>,
    },
    /// Replace the employee's custom role — the deliberate fix when its
    /// assigned persona became unavailable. The canonical Employee base
    /// always stays composed underneath; `None` drops to the base alone.
    /// The Boss persona is rejected, and instruction text refreshes on
    /// the next turn while recorded grants, pins, and icon stay as they
    /// were assigned.
    SetPersona {
        #[serde(default)]
        persona_id: Option<Uuid>,
    },
    Stop,
}

#[cfg(test)]
mod tests {
    use super::EmployeeControl;
    use uuid::Uuid;

    #[test]
    fn old_identity_keeps_moods_and_blobby_maps_to_gaze() {
        let identity: super::BossIdentity = serde_json::from_value(serde_json::json!({
            "id": Uuid::nil(), "name": "Boss", "avatarSeed": "existing"
        }))
        .unwrap();
        assert_eq!(identity.avatar_style, super::AvatarStyle::DiceBear);
        let mut value = serde_json::to_value(identity).unwrap();
        value["avatarStyle"] = serde_json::json!("blobby");
        let identity: super::BossIdentity = serde_json::from_value(value).unwrap();
        assert_eq!(identity.avatar_style, super::AvatarStyle::Gaze);
        assert_eq!(super::AvatarStyle::default(), super::AvatarStyle::Gaze);
    }

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
    fn set_model_control_tolerates_a_stale_interrupt_field() {
        // Clients pinned to the older wire shape send `interrupt` — the
        // flag is gone, and serde ignores it rather than rejecting.
        let action: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setModel",
            "provider": "codex",
            "model": "gpt-5.5",
            "interrupt": true
        }))
        .unwrap();
        assert!(matches!(
            action,
            EmployeeControl::SetModel { model, .. } if model == "gpt-5.5"
        ));
    }

    #[test]
    fn set_workspace_control_decodes_workspace_and_base_branch() {
        let action: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setWorkspace",
            "workspace": "worktree",
            "baseBranch": "dev"
        }))
        .unwrap();
        assert!(matches!(
            action,
            EmployeeControl::SetWorkspace { workspace, base_branch }
                if workspace == crate::AgentWorkspace::Worktree
                    && base_branch.as_deref() == Some("dev")
        ));
        let local: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setWorkspace",
            "workspace": "local"
        }))
        .unwrap();
        assert!(matches!(
            local,
            EmployeeControl::SetWorkspace { workspace, base_branch }
                if workspace == crate::AgentWorkspace::Local && base_branch.is_none()
        ));
    }

    #[test]
    fn set_resources_control_decodes_a_host_set() {
        let action: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setResources",
            "resources": {"exclusive": ["ios:ABC"], "resident_devices": 1, "native_builds": 1}
        }))
        .unwrap();
        assert!(matches!(
            action,
            EmployeeControl::SetResources { resources }
                if resources.native_builds == 1
                    && resources.resident_devices == 1
                    && resources.exclusive == ["ios:ABC".to_owned()]
        ));
        let empty: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setResources",
            "resources": {}
        }))
        .unwrap();
        assert!(matches!(
            empty,
            EmployeeControl::SetResources { resources }
                if resources == crate::resources::ResourceSet::default()
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
            "reasoningEffort": "high",
            "permissions": {"bucketIds": ["work"], "computerUse": true}
        }))
        .unwrap();
        assert!(matches!(
            summon,
            super::BossOperation::Summon { permissions: Some(overrides), reasoning_effort, work_goal, .. }
                if overrides.bucket_ids.as_deref() == Some(&["work".to_string()][..])
                    && overrides.computer_use == Some(true)
                    && overrides.integration_ids.is_none()
                    && reasoning_effort.as_deref() == Some("high")
                    // An omitted kind is an assignment: its finish reports.
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
            super::BossOperation::Summon {
                work_goal: super::EmployeeGoal::Goal,
                ..
            }
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
            "prompt": "Plan the auth migration",
            "reasoningEffort": "medium"
        }))
        .unwrap();
        assert!(matches!(
            create,
            super::BossOperation::CreatePlan {
                title, plan_file, prompt, provider, model, reasoning_effort
            }
                if title == "Auth migration" && plan_file == "auth.md"
                    && prompt == "Plan the auth migration"
                    && provider.is_none() && model.is_none()
                    && reasoning_effort.as_deref() == Some("medium")
        ));
        let terminal: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "terminal",
            "title": "Dev server",
            "cwd": "/work/app",
            "command": "bun run dev"
        }))
        .unwrap();
        assert!(matches!(
            terminal,
            super::BossOperation::Terminal { title, cwd, command }
                if title == "Dev server" && cwd == "/work/app" && command.as_deref() == Some("bun run dev")
        ));
        // A planning session finalizes its own plan by omitting the file.
        let finalize: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "finalizePlan"
        }))
        .unwrap();
        assert!(matches!(
            finalize,
            super::BossOperation::FinalizePlan {
                plan_file: None,
                items: None
            }
        ));
        let seeded: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "finalizePlan",
            "planFile": "plans/auth.md",
            "items": ["Probe", "Verify"]
        }))
        .unwrap();
        assert!(matches!(
            seeded,
            super::BossOperation::FinalizePlan {
                plan_file: Some(file),
                items: Some(items)
            } if file == "plans/auth.md" && items == ["Probe", "Verify"]
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
        assert!(
            matches!(result, super::BossResult::Browse { url, title: None, .. } if url == "https://example.com/docs")
        );
    }

    #[test]
    fn work_breakdown_operations_decode_camel_case_payloads() {
        let items: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "updatePlanItems",
            "plan": "plans/auth.md",
            "items": [{"title": "Probe"}, {"id": "00000000-0000-0000-0000-000000000002", "title": "Verify"}]
        }))
        .unwrap();
        assert!(matches!(
            items,
            super::BossOperation::UpdatePlanItems { plan, items }
                if plan == "plans/auth.md"
                    && items.len() == 2
                    && items[0].id.is_none()
                    && items[1].id == Some(Uuid::from_u128(2))
        ));
    }

    #[test]
    fn plan_tag_and_retag_operations_decode_camel_case_payloads() {
        let summon: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "summon",
            "personaId": "00000000-0000-0000-0000-000000000001",
            "jobTitle": "Worker",
            "prompt": "Do work",
            "project": "/tmp",
            "plan": "plans/auth.md",
            "item": "00000000-0000-0000-0000-000000000002"
        }))
        .unwrap();
        assert!(matches!(
            summon,
            super::BossOperation::Summon { plan, item, .. }
                if plan.as_deref() == Some("plans/auth.md")
                    && item == Some(Uuid::from_u128(2))
        ));
        // Absent keeps the link, null clears it, a value re-tags.
        let retag: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setPlan",
            "plan": "plans/auth.md",
            "item": "00000000-0000-0000-0000-000000000002"
        }))
        .unwrap();
        assert!(matches!(
            retag,
            EmployeeControl::SetPlan { plan, item }
                if plan == Some(Some("plans/auth.md".to_owned()))
                    && item == Some(Some(Uuid::from_u128(2)))
        ));
        let clear: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setPlan",
            "plan": null,
            "item": null
        }))
        .unwrap();
        assert!(matches!(
            clear,
            EmployeeControl::SetPlan { plan, item }
                if plan == Some(None) && item == Some(None)
        ));
        let keep: EmployeeControl = serde_json::from_value(serde_json::json!({
            "type": "setPlan"
        }))
        .unwrap();
        assert!(matches!(
            keep,
            EmployeeControl::SetPlan { plan, item }
                if plan.is_none() && item.is_none()
        ));
    }

    #[test]
    fn plan_lifecycle_operations_decode_camel_case_payloads() {
        let item_state: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "setPlanItemState",
            "plan": "plans/auth.md",
            "item": "00000000-0000-0000-0000-000000000002",
            "state": "dropped"
        }))
        .unwrap();
        assert!(matches!(
            item_state,
            super::BossOperation::SetPlanItemState { state, .. }
                if state == super::PlanItemState::Dropped
        ));
        let outcome: super::BossOperation = serde_json::from_value(serde_json::json!({
            "type": "setPlanOutcome",
            "plan": "plans/auth.md",
            "outcome": "completed"
        }))
        .unwrap();
        assert!(matches!(
            outcome,
            super::BossOperation::SetPlanOutcome { outcome, .. }
                if outcome == super::PlanOutcome::Completed
        ));
    }

    #[test]
    fn a_plan_record_predating_the_work_fields_still_decodes() {
        let plan: super::BossPlan = serde_json::from_value(serde_json::json!({
            "sessionId": "00000000-0000-0000-0000-000000000001",
            "planFile": "plans/auth.md",
            "idea": "Auth",
            "finalizedAt": 1_700_000_000
        }))
        .unwrap();
        assert!(!plan.id.is_nil());
        assert!(plan.items.is_empty());
        assert_eq!(plan.outcome(), super::PlanOutcome::Approved);
        assert_eq!(plan.lifecycle(), super::PlanLifecycle::Approved);
        assert!(plan.history.is_empty());
        let draft: super::BossPlan = serde_json::from_value(serde_json::json!({
            "sessionId": "00000000-0000-0000-0000-000000000001",
            "planFile": "plans/draft.md",
            "idea": "Draft"
        }))
        .unwrap();
        assert_eq!(draft.lifecycle(), super::PlanLifecycle::Planning);
    }

    #[test]
    fn boss_operations_reject_unknown_fields() {
        let error = serde_json::from_value::<super::BossOperation>(serde_json::json!({
            "type": "summon",
            "personaId": "00000000-0000-0000-0000-000000000001",
            "jobTitle": "Worker",
            "prompt": "Do work",
            "project": "/tmp",
            "misspelled": true
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
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
            "deliverables": [],
            "revision": 0
        }))
        .unwrap();
        assert!(state.planning.is_empty());
    }
}
