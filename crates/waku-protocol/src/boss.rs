//! Daemon-owned Boss identities, personas, files, and employee relationships.

use std::path::PathBuf;

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

    /// Whether reviving the job makes sense — only a supervisor stop is
    /// terminal intent; every other settle revives through `resume` or a
    /// plain prompt.
    pub fn resumable(self) -> bool {
        self != Self::Stopped
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
            Self::Failed | Self::ExitedMidTurn | Self::Restarted | Self::ParkedWork | Self::UnansweredAsk
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
    /// The daemon-owned goal this assignment projects onto, when the
    /// summoner linked one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub goal_id: Option<Uuid>,
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
    pub goal_id: Option<Uuid>,
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
}

impl BossEmployee {
    /// Move the employee to `lifecycle`, keeping the `expired` wire
    /// projection and its timestamp in step.
    pub fn set_lifecycle(&mut self, lifecycle: EmployeeLifecycle, now: u64) {
        self.state = lifecycle;
        self.expired = lifecycle == EmployeeLifecycle::Expired;
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
/// sidebar. `path` is absolute on the daemon's host — employees run in their
/// assigned project directory, so deliverables point outside the Boss files root.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BossDeliverable {
    pub id: Uuid,
    pub name: String,
    pub path: String,
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
    /// Documents saved before the rename record these under `bundles`.
    #[serde(default, alias = "bundles")]
    pub deliverables: Vec<BossDeliverable>,
    /// Open planning sessions and their plan documents. Records stay after
    /// finalization and archive — the freeze they carry is permanent.
    #[serde(default)]
    pub planning: Vec<BossPlan>,
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
        persona_id: Uuid,
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
        /// Link the assignment to a daemon-owned goal projection — the
        /// Goals page shows it as pending work before any provider thread
        /// exists.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        goal_id: Option<Uuid>,
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
        persona: BossPersonaUpsert,
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
    PublishDeliverable {
        path: String,
        #[serde(default)]
        name: Option<String>,
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
    Stop,
}

#[cfg(test)]
mod tests {
    use super::EmployeeControl;
    use uuid::Uuid;

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
        assert!(matches!(
            action,
            EmployeeControl::SetModel {
                interrupt: None,
                ..
            }
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
