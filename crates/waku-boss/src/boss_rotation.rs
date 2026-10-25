//! Store-independent policy and durable intent records for Boss rotation.
//!
//! Rotation defaults on: a settled Boss session rotates once its context
//! crosses the configured threshold. The daemon owns provider session
//! creation; this module makes the trigger and the journal decision
//! deterministic and restart-readable.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::rotation_oracle::{
    AdmissionDecision, CompactionStage, ContextSampleSource, RotationLifecycleStage,
};
use waku_protocol::boss::{BossRotationPhase, BossRotationStatus};
use waku_protocol::model::ProviderKind;

/// Seconds a pending intent may age before the delay becomes visible —
/// the design's delayed-refresh deadline.
pub const ROTATION_DELAYED_AFTER_SECS: u64 = 30;

/// Lifecycle signals retained on the journal — bounded so the document
/// stays a fixed size; the newest always wins the slot.
const ROTATION_SIGNAL_CAP: usize = 256;

/// What asked for the rotation — the machine-readable half of the
/// free-form `RotationIntent::reason`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RotationCause {
    /// Occupancy crossed the configured context threshold.
    ContextThreshold,
    /// A daemon or human requested refresh outside the occupancy trigger.
    Requested,
    /// Recovery after a lifecycle incident, including a detected
    /// provider compaction on the active session.
    Incident,
    /// The intent predates cause recording or could not be classified.
    #[default]
    Unknown,
}

/// The provider-reported context sample a rotation trigger evaluated —
/// the observed U input, separate from any bounded guard estimate.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationUsageSample {
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    /// Who supplied the numbers — a provider report or a conservative bound.
    pub source: ContextSampleSource,
}

/// The intake position the refresh froze against the retiring session —
/// the event boundary downstream reconciliation replays from.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationEventBoundary {
    pub session_id: Uuid,
    /// Persisted messages counted when the boundary was frozen.
    pub message_count: usize,
    /// The newest persisted message — the boundary's replay anchor.
    pub last_message_id: Option<Uuid>,
}

/// Canonical instruction identity carried through the rotation — a
/// digest names the exact revision; the text itself is never logged.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationInstruction {
    pub persona_id: Uuid,
    /// Shipped-default revision the effective instructions build on.
    pub base_revision: Option<u32>,
    /// SHA-256 of the effective persona markdown, hex encoded.
    pub digest: Option<String>,
}

/// Successor validation outcome for the handoff.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    tag = "type",
    rename_all_fields = "camelCase"
)]
pub enum RotationValidation {
    /// The lifecycle ran no successor validation — recorded explicitly
    /// rather than assumed.
    NotRun,
    Passed {
        at: u64,
    },
    Failed {
        at: u64,
        cause: String,
    },
}

/// How the rotation resolved. `Activated` is the only success terminal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    tag = "type",
    rename_all_fields = "camelCase"
)]
pub enum RotationTerminal {
    Activated { at: u64 },
    Blocked { at: u64, cause: String },
    Cancelled { at: u64, cause: String },
    Failed { at: u64, cause: String },
}

/// One timestamped lifecycle transition, in observed order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationStageMark {
    pub stage: RotationLifecycleStage,
    pub at: u64,
}

/// Everything a rotation can account for across the safety contract. Each
/// field reports what the lifecycle actually supplied — `None`/`Unguarded`
/// where the current lifecycle cannot measure it, never an implied bound.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RotationDiagnostics {
    pub cause: RotationCause,
    /// Provider/model/configuration the trigger ran under.
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub context_threshold: Option<f64>,
    /// Canonical instruction identity — digest, never text.
    pub instruction: Option<RotationInstruction>,
    /// The provider-reported sample the trigger evaluated (U evidence).
    pub usage: Option<RotationUsageSample>,
    /// The trigger-time guard decision — `Unguarded` while the lifecycle
    /// has no measured guard; a bounded decision lands with the guarded
    /// workstream.
    pub guard: AdmissionDecision,
    /// Computed headroom when the guard carried bounds.
    pub headroom: Option<u64>,
    /// The frozen intake/event boundary for reconciliation.
    pub event_boundary: Option<RotationEventBoundary>,
    /// Stage transitions in observed order.
    pub stages: Vec<RotationStageMark>,
    /// Time spent waiting on admission or capacity — `None` while Boss
    /// sessions bypass the employee admission queue.
    pub admission_wait_ms: Option<u64>,
    /// Successor validation outcome.
    pub validation: RotationValidation,
    /// Reconcile passes that failed mid-rotation, the last cause, and
    /// when it was recorded.
    pub failed_attempts: u32,
    pub last_error: Option<String>,
    pub last_error_at: Option<u64>,
    /// Terminal state once the rotation resolves.
    pub terminal: Option<RotationTerminal>,
}

impl Default for RotationDiagnostics {
    fn default() -> Self {
        Self {
            cause: RotationCause::Unknown,
            provider: None,
            model: None,
            context_threshold: None,
            instruction: None,
            usage: None,
            guard: AdmissionDecision::Unguarded,
            headroom: None,
            event_boundary: None,
            stages: Vec::new(),
            admission_wait_ms: None,
            validation: RotationValidation::NotRun,
            failed_attempts: 0,
            last_error: None,
            last_error_at: None,
            terminal: None,
        }
    }
}

impl RotationDiagnostics {
    /// Append a stage transition; back-to-back marks of the same stage do
    /// not duplicate — a retried stage is not a new transition.
    fn mark(&mut self, stage: RotationLifecycleStage, at: u64) {
        if self.stages.last().is_some_and(|mark| mark.stage == stage) {
            return;
        }
        self.stages.push(RotationStageMark { stage, at });
    }
}

/// Where a compaction signal originated.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompactionOrigin {
    /// A `compact` command the daemon issued — recorded as an attempt,
    /// never honored as prevention.
    DaemonCommand,
    /// Provider telemetry reported the compaction itself.
    ProviderAutonomous,
    /// The signal's source could not be established.
    Unknown,
}

/// A provider compaction observed on the active Boss session. The
/// incident stays surfaced until a committed rotation replaces the owner.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionIncident {
    pub at: u64,
    pub session_id: Uuid,
    pub generation: Option<u64>,
    pub stage: CompactionStage,
}

/// Cross-rotation lifecycle evidence the journal retains — compaction
/// events and explicit telemetry gaps, correlated to generation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    tag = "type",
    rename_all_fields = "camelCase"
)]
pub enum RotationSignal {
    /// A compaction attempt, start, completion, or failure.
    Compaction {
        at: u64,
        session_id: Uuid,
        runtime_id: Option<Uuid>,
        /// The session's owner generation when the signal landed.
        generation: Option<u64>,
        stage: CompactionStage,
        origin: CompactionOrigin,
        /// The rotation pending when the signal landed, for correlation.
        rotation_id: Option<Uuid>,
    },
    /// A generation closed without any observed compaction telemetry —
    /// recorded explicitly; silence is not evidence of absence.
    CompactionTelemetryGap {
        at: u64,
        session_id: Uuid,
        generation: Option<u64>,
        rotation_id: Option<Uuid>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BossRotationConfig {
    /// Rotate once context reaches this fraction of the reported window.
    pub context_threshold: f64,
}

impl Default for BossRotationConfig {
    fn default() -> Self {
        Self {
            context_threshold: 0.75,
        }
    }
}

impl BossRotationConfig {
    pub fn should_rotate(&self, context_tokens: u64, context_window: Option<u64>) -> bool {
        if !self.context_threshold.is_finite() {
            return false;
        }
        let Some(window) = context_window.filter(|window| *window > 0) else {
            return false;
        };
        context_tokens as f64 / window as f64 > self.context_threshold.clamp(0.0, 1.0)
    }
}

/// A replayable rotation intent. `new_session_id` is only published after the
/// caller has durably created and initialized the provider session.
/// Older checkpoints retain the strict receipt/budget contract until recovery
/// proves that no replacement work was submitted. Ordinary refreshes are lazy
/// session swaps and make no provider-compaction prevention guarantee.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RotationMode {
    #[default]
    Strict,
    BestEffort,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationIntent {
    #[serde(default)]
    pub mode: RotationMode,
    pub boss_id: Uuid,
    pub old_session_id: Uuid,
    pub old_generation: u64,
    pub new_session_id: Uuid,
    pub created_at: u64,
    pub committed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The authorization-bearing identity revision bound when the intent was
    /// persisted. The successor may publish only while the authoritative
    /// document still digests to this value — a persona, policy or grant
    /// change during staging aborts the rotation rather than activating
    /// stale grants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_digest: Option<String>,
    /// The provider/model configuration this rotation was staged under,
    /// recorded for telemetry and rebound validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The prevention-contract evidence held for `provider` at intent time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_qualification: Option<crate::rotation_continuity::ProviderQualification>,
    /// The lifecycle status and diagnostics record — what the rotation
    /// measured, decided, and reached. Missing on pre-contract intents.
    #[serde(default)]
    pub diagnostics: RotationDiagnostics,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RotationJournal {
    pub generation: u64,
    pub active_session_id: Option<Uuid>,
    pub intent: Option<RotationIntent>,
    /// Completed swaps retained for diagnostics and transcript provenance.
    pub rotations: Vec<RotationIntent>,
    /// Cross-rotation lifecycle evidence — compaction events and explicit
    /// telemetry gaps, bounded so the document stays a fixed size.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<RotationSignal>,
    /// A provider compaction observed on the active session — surfaced
    /// until a committed rotation installs a clean generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident: Option<CompactionIncident>,
}

impl RotationJournal {
    /// Persist an intent before staging the new session. Existing unresolved
    /// intents are rejected so recovery cannot silently lose a generation.
    pub fn begin(
        &mut self,
        boss_id: Uuid,
        old_session_id: Uuid,
        new_session_id: Uuid,
        now: u64,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.intent.is_none(), "a Boss rotation is already pending");
        anyhow::ensure!(
            self.active_session_id == Some(old_session_id),
            "Boss session pointer changed before rotation"
        );
        self.intent = Some(RotationIntent {
            mode: RotationMode::Strict,
            boss_id,
            old_session_id,
            old_generation: self.generation,
            new_session_id,
            created_at: now,
            committed_at: None,
            reason: None,
            identity_digest: None,
            provider: None,
            model: None,
            provider_qualification: None,
            diagnostics: RotationDiagnostics::default(),
        });
        Ok(())
    }

    /// Complete the compare-and-swap after session initialization succeeds.
    /// In the current lifecycle the committed owner is immediately the
    /// coordinator — activation is not yet a distinct validated step.
    pub fn commit(&mut self, now: u64) -> anyhow::Result<Uuid> {
        let intent = self
            .intent
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no Boss rotation is pending"))?;
        anyhow::ensure!(
            self.generation == intent.old_generation
                && self.active_session_id == Some(intent.old_session_id),
            "Boss rotation lost its generation compare-and-swap"
        );
        intent.committed_at = Some(now);
        intent
            .diagnostics
            .mark(RotationLifecycleStage::OwnershipCommitted, now);
        intent
            .diagnostics
            .mark(RotationLifecycleStage::SuccessorActivated, now);
        intent.diagnostics.terminal = Some(RotationTerminal::Activated { at: now });
        let next = intent.new_session_id;
        self.active_session_id = Some(next);
        self.generation = self.generation.saturating_add(1);
        self.rotations.push(intent.clone());
        self.intent = None;
        // A committed generation closes any incident the retired owner
        // carried — whatever replaces it starts with a clean record.
        self.incident = None;
        Ok(next)
    }

    /// Abandon an uncommitted staging attempt. The old pointer stays active.
    pub fn abort(&mut self) {
        self.intent = None;
    }

    /// The pending rotation's stable identity — its successor session id.
    pub fn pending_rotation_id(&self) -> Option<Uuid> {
        self.intent.as_ref().map(|intent| intent.new_session_id)
    }

    /// Append a lifecycle transition to the pending intent's diagnostics.
    pub fn record_stage(&mut self, stage: RotationLifecycleStage, at: u64) {
        if let Some(intent) = &mut self.intent {
            intent.diagnostics.mark(stage, at);
        }
    }

    /// Record a reconcile failure against the pending intent — the paused
    /// status surfaces its cause until the rotation resolves.
    pub fn record_attempt_failure(&mut self, cause: String, at: u64) {
        if let Some(intent) = &mut self.intent {
            intent.diagnostics.failed_attempts =
                intent.diagnostics.failed_attempts.saturating_add(1);
            intent.diagnostics.last_error = Some(cause);
            intent.diagnostics.last_error_at = Some(at);
        }
    }

    /// The generation a session owns or owned under this journal —
    /// the correlation key every lifecycle signal carries.
    pub fn generation_for(&self, session_id: Uuid) -> Option<u64> {
        if let Some(intent) = &self.intent {
            if intent.new_session_id == session_id {
                return Some(intent.old_generation.saturating_add(1));
            }
            if intent.old_session_id == session_id {
                return Some(intent.old_generation);
            }
        }
        if self.active_session_id == Some(session_id) {
            return Some(self.generation);
        }
        self.rotations.iter().rev().find_map(|rotation| {
            if rotation.new_session_id == session_id {
                Some(rotation.old_generation.saturating_add(1))
            } else if rotation.old_session_id == session_id {
                Some(rotation.old_generation)
            } else {
                None
            }
        })
    }

    /// Append a lifecycle signal, dropping the oldest past the cap.
    pub fn record_signal(&mut self, signal: RotationSignal) {
        if self.signals.len() >= ROTATION_SIGNAL_CAP {
            self.signals
                .drain(..self.signals.len() - (ROTATION_SIGNAL_CAP - 1));
        }
        self.signals.push(signal);
    }

    /// Record a provider compaction against a Boss session. The event
    /// lands in `signals` correlated to generation; while the affected
    /// session still owns coordination, the incident also stays surfaced
    /// until a committed rotation replaces it.
    pub fn record_compaction(
        &mut self,
        session_id: Uuid,
        runtime_id: Option<Uuid>,
        stage: CompactionStage,
        origin: CompactionOrigin,
        at: u64,
    ) {
        // Before the first committed rotation the pointer is unset — and an
        // intent can only exist once it is set — so a Boss-session signal can
        // only belong to the active owner. Seeding here also lets the signal
        // carry the session's true generation zero instead of `None`.
        if self.active_session_id.is_none() {
            self.active_session_id = Some(session_id);
        }
        let generation = self.generation_for(session_id);
        let rotation_id = self.pending_rotation_id();
        let still_owns = self.active_session_id == Some(session_id);
        self.record_signal(RotationSignal::Compaction {
            at,
            session_id,
            runtime_id,
            generation,
            stage,
            origin,
            rotation_id,
        });
        if still_owns && matches!(stage, CompactionStage::Started | CompactionStage::Completed) {
            self.incident = Some(CompactionIncident {
                at,
                session_id,
                generation,
                stage,
            });
        }
    }

    /// Whether any compaction telemetry was observed for a session —
    /// used to close a generation with an explicit gap record rather
    /// than leaving silence.
    pub fn observed_compaction_telemetry(&self, session_id: Uuid) -> bool {
        self.signals.iter().any(|signal| {
            matches!(
                signal,
                RotationSignal::Compaction {
                    session_id: recorded,
                    ..
                } if *recorded == session_id
            )
        })
    }

    /// The lifecycle position for the visible status surface — the
    /// pending rotation's phase, else a compaction incident on the
    /// active owner.
    pub fn status(&self, now: u64) -> Option<BossRotationStatus> {
        if let Some(intent) = &self.intent {
            let (phase, cause) = match &intent.diagnostics.last_error {
                Some(cause) => (BossRotationPhase::Paused, Some(bound_cause(cause))),
                None if now.saturating_sub(intent.created_at) >= ROTATION_DELAYED_AFTER_SECS => {
                    (BossRotationPhase::Delayed, None)
                }
                None => (BossRotationPhase::Refreshing, None),
            };
            return Some(BossRotationStatus {
                phase,
                since: intent.created_at,
                generation: intent.old_generation.saturating_add(1),
                rotation_id: Some(intent.new_session_id),
                cause,
            });
        }
        if let Some(incident) = &self.incident {
            return Some(BossRotationStatus {
                phase: BossRotationPhase::CompactionIncident,
                since: incident.at,
                generation: incident.generation.unwrap_or(self.generation),
                rotation_id: None,
                cause: Some("provider reported context compaction".to_owned()),
            });
        }
        None
    }

    pub fn persist(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        let temp = temporary_path(path);
        fs::write(&temp, bytes)?;
        fs::rename(temp, path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
}

/// The paused status's cause line — bounded so a chained error cannot
/// fill the banner, and never truncated mid-character.
fn bound_cause(cause: &str) -> String {
    const MAX_CAUSE_CHARS: usize = 240;
    if cause.len() <= MAX_CAUSE_CHARS {
        return cause.to_owned();
    }
    let mut end = MAX_CAUSE_CHARS;
    while !cause.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &cause[..end])
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!("rotation-{}.tmp", Uuid::new_v4().simple()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_requires_threshold_crossing_only() {
        let config = BossRotationConfig::default();
        assert!(!config.should_rotate(74, Some(100)));
        assert!(!config.should_rotate(75, Some(100)));
        assert!(config.should_rotate(76, Some(100)));
        assert!(!config.should_rotate(90, None));
        assert!(!config.should_rotate(90, Some(0)));
        let disabled = BossRotationConfig {
            context_threshold: f64::NAN,
        };
        assert!(!disabled.should_rotate(100, Some(100)));
    }

    #[test]
    fn journal_keeps_old_pointer_until_commit_and_survives_restart() {
        let root = std::env::temp_dir().join(format!("boss-rotation-{}", Uuid::new_v4()));
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        let boss = Uuid::new_v4();
        let mut journal = RotationJournal {
            active_session_id: Some(old),
            ..Default::default()
        };
        journal.begin(boss, old, next, 10).unwrap();
        journal.persist(&root).unwrap();
        let mut recovered = RotationJournal::load(&root).unwrap();
        assert_eq!(recovered.active_session_id, Some(old));
        assert!(recovered.intent.is_some());
        assert_eq!(recovered.commit(20).unwrap(), next);
        assert_eq!(recovered.generation, 1);
        assert_eq!(recovered.active_session_id, Some(next));
        recovered.persist(&root).unwrap();
        assert_eq!(
            RotationJournal::load(&root).unwrap().active_session_id,
            Some(next)
        );
        fs::remove_file(root).ok();
    }

    fn pending_journal() -> (RotationJournal, Uuid, Uuid) {
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        let mut journal = RotationJournal {
            active_session_id: Some(old),
            ..Default::default()
        };
        journal.begin(Uuid::new_v4(), old, next, 10).unwrap();
        (journal, old, next)
    }

    #[test]
    fn commit_marks_terminal_and_clears_the_incident() {
        let (mut journal, old, next) = pending_journal();
        journal.record_stage(RotationLifecycleStage::IntentPersisted, 11);
        journal.record_stage(RotationLifecycleStage::IntentPersisted, 12);
        journal.record_compaction(
            old,
            None,
            CompactionStage::Completed,
            CompactionOrigin::ProviderAutonomous,
            12,
        );
        assert!(journal.incident.is_some());
        assert_eq!(journal.generation_for(old), Some(0));
        assert_eq!(journal.generation_for(next), Some(1));
        journal.commit(20).unwrap();
        let done = journal.rotations.last().expect("committed rotation");
        let stages: Vec<_> = done
            .diagnostics
            .stages
            .iter()
            .map(|mark| mark.stage)
            .collect();
        // Back-to-back marks of one stage collapse; the commit appends
        // the terminal pair itself.
        assert_eq!(
            stages,
            [
                RotationLifecycleStage::IntentPersisted,
                RotationLifecycleStage::OwnershipCommitted,
                RotationLifecycleStage::SuccessorActivated,
            ]
        );
        assert_eq!(
            done.diagnostics.terminal,
            Some(RotationTerminal::Activated { at: 20 })
        );
        assert!(journal.incident.is_none());
    }

    #[test]
    fn status_reports_refresh_delay_pause_and_incident() {
        let (mut journal, old, next) = pending_journal();
        assert_eq!(RotationJournal::default().status(9), None);
        // Inside the delay window a pending refresh stays quiet.
        let status = journal.status(30).expect("pending status");
        assert_eq!(status.phase, BossRotationPhase::Refreshing);
        assert_eq!(status.rotation_id, Some(next));
        assert_eq!(status.generation, 1);
        // Past the deadline the same intent reads delayed, not failed.
        assert_eq!(
            journal.status(41).unwrap().phase,
            BossRotationPhase::Delayed
        );
        journal.record_attempt_failure("could not persist journal".into(), 42);
        let paused = journal.status(43).unwrap();
        assert_eq!(paused.phase, BossRotationPhase::Paused);
        assert_eq!(paused.cause.as_deref(), Some("could not persist journal"));
        journal.abort();
        // A fenced session's compaction records no incident; the active
        // owner's does.
        journal.record_compaction(
            next,
            None,
            CompactionStage::Started,
            CompactionOrigin::ProviderAutonomous,
            50,
        );
        assert!(journal.incident.is_none());
        journal.record_compaction(
            old,
            None,
            CompactionStage::Completed,
            CompactionOrigin::ProviderAutonomous,
            51,
        );
        let incident = journal.status(52).unwrap();
        assert_eq!(incident.phase, BossRotationPhase::CompactionIncident);
        assert_eq!(incident.generation, 0);
    }

    #[test]
    fn signals_stay_bounded_and_gap_is_explicit() {
        let (mut journal, old, _) = pending_journal();
        journal.abort();
        assert!(!journal.observed_compaction_telemetry(old));
        for index in 0..(ROTATION_SIGNAL_CAP + 20) {
            journal.record_signal(RotationSignal::CompactionTelemetryGap {
                at: index as u64,
                session_id: old,
                generation: Some(0),
                rotation_id: None,
            });
        }
        assert_eq!(journal.signals.len(), ROTATION_SIGNAL_CAP);
        journal.record_compaction(
            old,
            None,
            CompactionStage::Attempted,
            CompactionOrigin::DaemonCommand,
            300,
        );
        assert!(journal.observed_compaction_telemetry(old));
        assert!(!journal.observed_compaction_telemetry(Uuid::new_v4()));
    }

    #[test]
    fn legacy_intent_without_diagnostics_deserializes() {
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        let raw = format!(
            "{{\"generation\":0,\"activeSessionId\":\"{old}\",\"intent\":{{\"bossId\":\"{}\",\"oldSessionId\":\"{old}\",\"oldGeneration\":0,\"newSessionId\":\"{next}\",\"createdAt\":1,\"committedAt\":null}},\"rotations\":[]}}",
            Uuid::new_v4()
        );
        let journal: RotationJournal = serde_json::from_str(&raw).unwrap();
        let intent = journal.intent.unwrap();
        assert_eq!(intent.diagnostics.cause, RotationCause::Unknown);
        assert_eq!(intent.diagnostics.guard, AdmissionDecision::Unguarded);
        assert_eq!(intent.diagnostics.validation, RotationValidation::NotRun);
    }
}
