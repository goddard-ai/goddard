//! Independent observations for the Boss pre-compaction rotation guarantee.
//!
//! The oracle consumes seams from the real daemon/provider boundary. It never
//! issues provider work and does not treat a lifecycle request as proof that a
//! safe rotation happened.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The context-growing operation whose admission is being observed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AdmissionBoundary {
    FirstRequest,
    Request,
    Continuation,
    Resume,
    Replay,
    ProviderAutonomous,
}

/// A provider/model bound that must satisfy `U + G + M < C` before admission.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBounds {
    /// Verified lower bound at which provider compaction could begin (C).
    pub compaction_floor: u64,
    /// Conservative upper bound for effective context already present (U).
    pub current_context: u64,
    /// Maximum growth possible before the next enforced guard (G).
    pub max_growth: u64,
    /// Positive bound for counting and provider uncertainty (M).
    pub safety_margin: u64,
}

impl ContextBounds {
    pub fn projected_context(&self) -> Option<u64> {
        self.current_context
            .checked_add(self.max_growth)?
            .checked_add(self.safety_margin)
    }

    pub fn headroom(&self) -> Option<u64> {
        self.compaction_floor.checked_sub(self.projected_context()?)
    }

    pub fn admits(&self) -> bool {
        self.projected_context()
            .is_some_and(|projected| projected < self.compaction_floor)
    }
}

/// The decision made at the daemon/provider admission boundary.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AdmissionDecision {
    /// A guard evaluated the bound and admitted the operation.
    Admitted { bounds: ContextBounds },
    /// A guard stopped this operation; rotation or recovery may continue.
    Blocked { bounds: Option<ContextBounds> },
    /// The operation reached the provider without a measured guard.
    Unguarded,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderTransportStatus {
    Submitted,
    Accepted,
    Rejected,
    Recovered,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompactionStage {
    Attempted,
    Started,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompactionTelemetry {
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContextSampleSource {
    ProviderReport,
    ConservativeBound,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OwnershipState {
    /// Durable sole coordinator for this Boss generation.
    Active,
    /// A successor pointer is visible but has not been committed as owner.
    Published,
    /// The session can no longer admit context-growing work.
    Fenced,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RotationLifecycleStage {
    Triggered,
    IntakeFrozen,
    IntentPersisted,
    SuccessorPrepared,
    PointerPublished,
    PredecessorFenced,
    OwnershipCommitted,
    SuccessorActivated,
    /// The daemon recovered the rotation record after a process boundary.
    Recovered,
    RecoveryPaused,
    Blocked,
    Cancelled,
    Failed,
}

impl RotationLifecycleStage {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::SuccessorActivated | Self::Blocked | Self::Cancelled | Self::Failed
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DurableActionKind {
    PersistedIntent,
    SuccessorPreparation,
    OwnershipCommit,
    ProviderDelivery,
    ProviderEffect,
    EmployeeDispatch,
    Other,
}

impl DurableActionKind {
    fn requires_active_owner(self) -> bool {
        matches!(
            self,
            Self::ProviderDelivery | Self::ProviderEffect | Self::EmployeeDispatch | Self::Other
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DurableActionStatus {
    Prepared,
    Submitted,
    Accepted,
    Rejected,
    Recovered,
    Committed,
    Unresolved,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAdmission {
    pub operation_id: String,
    pub session_id: Uuid,
    pub runtime_id: Uuid,
    pub owner_generation: Option<u64>,
    pub boundary: AdmissionBoundary,
    pub decision: AdmissionDecision,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTransport {
    pub delivery_id: String,
    pub session_id: Uuid,
    pub runtime_id: Uuid,
    pub owner_generation: Option<u64>,
    pub boundary: AdmissionBoundary,
    pub status: ProviderTransportStatus,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCompaction {
    pub session_id: Uuid,
    pub runtime_id: Uuid,
    pub owner_generation: Option<u64>,
    pub stage: CompactionStage,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionTelemetryObservation {
    pub session_id: Uuid,
    pub owner_generation: Option<u64>,
    pub telemetry: CompactionTelemetry,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSample {
    pub session_id: Uuid,
    pub runtime_id: Uuid,
    pub owner_generation: Option<u64>,
    pub context_tokens: u64,
    pub context_window: Option<u64>,
    pub source: ContextSampleSource,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableAction {
    pub action_id: String,
    pub session_id: Option<Uuid>,
    pub owner_generation: Option<u64>,
    pub kind: DurableActionKind,
    pub status: DurableActionStatus,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipObservation {
    pub session_id: Uuid,
    pub generation: u64,
    pub state: OwnershipState,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleObservation {
    pub rotation_id: Uuid,
    pub old_session_id: Uuid,
    pub new_session_id: Uuid,
    pub old_generation: u64,
    pub new_generation: u64,
    pub stage: RotationLifecycleStage,
}

/// One independently observed signal. Implementations may emit a subset, but
/// missing ownership/admission evidence remains a violation rather than an
/// assumed pass.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RotationObservation {
    ProviderAdmission(ProviderAdmission),
    ProviderTransport(ProviderTransport),
    ProviderCompaction(ProviderCompaction),
    CompactionTelemetry(CompactionTelemetryObservation),
    ContextSample(ContextSample),
    DurableAction(DurableAction),
    Ownership(OwnershipObservation),
    Lifecycle(LifecycleObservation),
}

/// Contract evidence supplied by provider qualification. A verified
/// no-compaction contract can substitute for telemetry; it cannot substitute
/// for bounded context admission.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderQualification {
    pub verified_no_compaction: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OracleViolation {
    MissingAdmissionGuard {
        session_id: Uuid,
        runtime_id: Uuid,
        boundary: AdmissionBoundary,
    },
    UnsafeAdmission {
        session_id: Uuid,
        runtime_id: Uuid,
        boundary: AdmissionBoundary,
        projected_context: Option<u64>,
        compaction_floor: u64,
    },
    MissingOwnership {
        session_id: Uuid,
        observation: String,
    },
    InactiveSession {
        session_id: Uuid,
        state: OwnershipState,
        observation: String,
    },
    StaleGeneration {
        session_id: Uuid,
        active_generation: u64,
        observed_generation: u64,
        observation: String,
    },
    OwnershipConflict {
        active_session_id: Uuid,
        active_generation: u64,
        attempted_session_id: Uuid,
        attempted_generation: u64,
    },
    ReactivatedFencedSession {
        session_id: Uuid,
        generation: u64,
    },
    CompactionEntered {
        session_id: Uuid,
        runtime_id: Uuid,
        stage: CompactionStage,
    },
    CompactionTelemetryUnavailable {
        session_id: Uuid,
    },
    DuplicateProviderEffect {
        action_id: String,
    },
    InvalidLifecycleTransition {
        rotation_id: Uuid,
        from: Option<RotationLifecycleStage>,
        to: RotationLifecycleStage,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OracleReport {
    pub admissions: usize,
    pub admitted_operations: usize,
    pub blocked_admissions: usize,
    pub unsafe_admissions: usize,
    pub provider_submissions: usize,
    pub provider_acceptances: usize,
    pub provider_rejections: usize,
    pub recovered_deliveries: usize,
    pub pending_deliveries: Vec<String>,
    pub compactions: usize,
    pub context_samples: usize,
    pub durable_actions: usize,
    pub provider_effects: usize,
    pub rotations: usize,
    pub completed_rotations: usize,
    pub terminal_rotations: usize,
    pub ownership_observations: usize,
    pub min_headroom: Option<u64>,
    pub violations: Vec<OracleViolation>,
}

impl Default for OracleReport {
    fn default() -> Self {
        Self {
            admissions: 0,
            admitted_operations: 0,
            blocked_admissions: 0,
            unsafe_admissions: 0,
            provider_submissions: 0,
            provider_acceptances: 0,
            provider_rejections: 0,
            recovered_deliveries: 0,
            pending_deliveries: Vec::new(),
            compactions: 0,
            context_samples: 0,
            durable_actions: 0,
            provider_effects: 0,
            rotations: 0,
            completed_rotations: 0,
            terminal_rotations: 0,
            ownership_observations: 0,
            min_headroom: None,
            violations: Vec::new(),
        }
    }
}

impl OracleReport {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Stateful event reducer used by deterministic and integrated acceptance
/// tests. A trace can be replayed after a simulated daemon restart by creating
/// a fresh oracle and feeding the durable observations again.
pub struct RotationOracle {
    qualification: ProviderQualification,
    owner_states: BTreeMap<Uuid, (u64, OwnershipState)>,
    active_owner: Option<(Uuid, u64)>,
    lifecycle: BTreeMap<Uuid, RotationLifecycleStage>,
    deliveries: BTreeMap<String, ProviderTransportStatus>,
    provider_effects: BTreeSet<String>,
    admissions: Vec<ProviderAdmission>,
    transports: Vec<ProviderTransport>,
    compactions: Vec<ProviderCompaction>,
    context_samples: Vec<ContextSample>,
    durable_actions: Vec<DurableAction>,
    ownership: Vec<OwnershipObservation>,
    lifecycle_events: Vec<LifecycleObservation>,
    violations: Vec<OracleViolation>,
    min_headroom: Option<u64>,
}

impl Default for RotationOracle {
    fn default() -> Self {
        Self::new(ProviderQualification::default())
    }
}

impl RotationOracle {
    pub fn new(qualification: ProviderQualification) -> Self {
        Self {
            qualification,
            owner_states: BTreeMap::new(),
            active_owner: None,
            lifecycle: BTreeMap::new(),
            deliveries: BTreeMap::new(),
            provider_effects: BTreeSet::new(),
            admissions: Vec::new(),
            transports: Vec::new(),
            compactions: Vec::new(),
            context_samples: Vec::new(),
            durable_actions: Vec::new(),
            ownership: Vec::new(),
            lifecycle_events: Vec::new(),
            violations: Vec::new(),
            min_headroom: None,
        }
    }

    pub fn observe(&mut self, observation: RotationObservation) {
        match observation {
            RotationObservation::ProviderAdmission(observation) => {
                self.observe_admission(observation);
            }
            RotationObservation::ProviderTransport(observation) => {
                self.observe_transport(observation);
            }
            RotationObservation::ProviderCompaction(observation) => {
                self.violations.push(OracleViolation::CompactionEntered {
                    session_id: observation.session_id,
                    runtime_id: observation.runtime_id,
                    stage: observation.stage,
                });
                self.compactions.push(observation);
            }
            RotationObservation::CompactionTelemetry(observation) => {
                if observation.telemetry == CompactionTelemetry::Unavailable
                    && !self.qualification.verified_no_compaction
                {
                    self.violations
                        .push(OracleViolation::CompactionTelemetryUnavailable {
                            session_id: observation.session_id,
                        });
                }
            }
            RotationObservation::ContextSample(observation) => {
                self.context_samples.push(observation);
            }
            RotationObservation::DurableAction(observation) => {
                self.observe_durable_action(observation);
            }
            RotationObservation::Ownership(observation) => {
                self.observe_ownership(observation);
            }
            RotationObservation::Lifecycle(observation) => {
                self.observe_lifecycle(observation);
            }
        }
    }

    pub fn evaluate(observations: impl IntoIterator<Item = RotationObservation>) -> OracleReport {
        Self::evaluate_with(ProviderQualification::default(), observations)
    }

    pub fn evaluate_with(
        qualification: ProviderQualification,
        observations: impl IntoIterator<Item = RotationObservation>,
    ) -> OracleReport {
        let mut oracle = Self::new(qualification);
        for observation in observations {
            oracle.observe(observation);
        }
        oracle.report()
    }

    pub fn report(&self) -> OracleReport {
        let terminal_rotations = self
            .lifecycle
            .values()
            .filter(|stage| stage.terminal())
            .count();
        OracleReport {
            admissions: self.admissions.len(),
            admitted_operations: self
                .admissions
                .iter()
                .filter(|admission| {
                    matches!(admission.decision, AdmissionDecision::Admitted { .. })
                })
                .count(),
            blocked_admissions: self
                .admissions
                .iter()
                .filter(|admission| matches!(admission.decision, AdmissionDecision::Blocked { .. }))
                .count(),
            unsafe_admissions: self
                .violations
                .iter()
                .filter(|violation| {
                    matches!(
                        violation,
                        OracleViolation::MissingAdmissionGuard { .. }
                            | OracleViolation::UnsafeAdmission { .. }
                    )
                })
                .count(),
            provider_submissions: self
                .transports
                .iter()
                .filter(|transport| transport.status == ProviderTransportStatus::Submitted)
                .count(),
            provider_acceptances: self
                .transports
                .iter()
                .filter(|transport| transport.status == ProviderTransportStatus::Accepted)
                .count(),
            provider_rejections: self
                .transports
                .iter()
                .filter(|transport| transport.status == ProviderTransportStatus::Rejected)
                .count(),
            recovered_deliveries: self
                .transports
                .iter()
                .filter(|transport| transport.status == ProviderTransportStatus::Recovered)
                .count(),
            pending_deliveries: self
                .deliveries
                .iter()
                .filter(|(_, status)| {
                    matches!(
                        status,
                        ProviderTransportStatus::Submitted
                            | ProviderTransportStatus::Rejected
                            | ProviderTransportStatus::Recovered
                    )
                })
                .map(|(delivery_id, _)| delivery_id.clone())
                .collect(),
            compactions: self.compactions.len(),
            context_samples: self.context_samples.len(),
            durable_actions: self.durable_actions.len(),
            provider_effects: self.provider_effects.len(),
            rotations: self.lifecycle.len(),
            completed_rotations: self
                .lifecycle
                .values()
                .filter(|stage| **stage == RotationLifecycleStage::SuccessorActivated)
                .count(),
            terminal_rotations,
            ownership_observations: self.ownership.len(),
            min_headroom: self.min_headroom,
            violations: self.violations.clone(),
        }
    }

    fn observe_admission(&mut self, observation: ProviderAdmission) {
        self.require_owner(
            observation.session_id,
            observation.owner_generation,
            "provider admission",
        );
        match &observation.decision {
            AdmissionDecision::Admitted { bounds } => {
                if bounds.admits() {
                    if let Some(headroom) = bounds.headroom() {
                        self.min_headroom =
                            Some(self.min_headroom.map_or(headroom, |min| min.min(headroom)));
                    }
                } else {
                    self.violations.push(OracleViolation::UnsafeAdmission {
                        session_id: observation.session_id,
                        runtime_id: observation.runtime_id,
                        boundary: observation.boundary,
                        projected_context: bounds.projected_context(),
                        compaction_floor: bounds.compaction_floor,
                    });
                }
            }
            AdmissionDecision::Blocked { .. } => {}
            AdmissionDecision::Unguarded => {
                self.violations
                    .push(OracleViolation::MissingAdmissionGuard {
                        session_id: observation.session_id,
                        runtime_id: observation.runtime_id,
                        boundary: observation.boundary,
                    });
            }
        }
        self.admissions.push(observation);
    }

    fn observe_transport(&mut self, observation: ProviderTransport) {
        self.require_owner(
            observation.session_id,
            observation.owner_generation,
            "provider transport",
        );
        self.deliveries
            .insert(observation.delivery_id.clone(), observation.status);
        self.transports.push(observation);
    }

    fn observe_durable_action(&mut self, observation: DurableAction) {
        if observation.kind.requires_active_owner()
            && let Some(session_id) = observation.session_id
        {
            self.require_owner(session_id, observation.owner_generation, "durable action");
        }
        if observation.kind == DurableActionKind::ProviderEffect
            && observation.status == DurableActionStatus::Committed
            && !self.provider_effects.insert(observation.action_id.clone())
        {
            self.violations
                .push(OracleViolation::DuplicateProviderEffect {
                    action_id: observation.action_id.clone(),
                });
        }
        self.durable_actions.push(observation);
    }

    fn observe_ownership(&mut self, observation: OwnershipObservation) {
        let previous = self.owner_states.get(&observation.session_id).copied();
        match observation.state {
            OwnershipState::Active => {
                if previous.is_some_and(|(_, state)| state == OwnershipState::Fenced) {
                    self.violations
                        .push(OracleViolation::ReactivatedFencedSession {
                            session_id: observation.session_id,
                            generation: observation.generation,
                        });
                }
                if let Some((session_id, generation)) = self.active_owner
                    && (session_id, generation) != (observation.session_id, observation.generation)
                {
                    self.violations.push(OracleViolation::OwnershipConflict {
                        active_session_id: session_id,
                        active_generation: generation,
                        attempted_session_id: observation.session_id,
                        attempted_generation: observation.generation,
                    });
                }
                self.active_owner = Some((observation.session_id, observation.generation));
            }
            OwnershipState::Published => {
                if previous.is_some_and(|(_, state)| state == OwnershipState::Fenced) {
                    self.violations
                        .push(OracleViolation::ReactivatedFencedSession {
                            session_id: observation.session_id,
                            generation: observation.generation,
                        });
                }
            }
            OwnershipState::Fenced => {
                if self.active_owner == Some((observation.session_id, observation.generation)) {
                    self.active_owner = None;
                }
            }
        }
        self.owner_states.insert(
            observation.session_id,
            (observation.generation, observation.state),
        );
        self.ownership.push(observation);
    }

    fn observe_lifecycle(&mut self, observation: LifecycleObservation) {
        let from = self.lifecycle.get(&observation.rotation_id).copied();
        if observation.stage != RotationLifecycleStage::Recovered {
            if let Some(from) = from {
                if from.terminal() && observation.stage != from {
                    self.violations
                        .push(OracleViolation::InvalidLifecycleTransition {
                            rotation_id: observation.rotation_id,
                            from: Some(from),
                            to: observation.stage,
                        });
                } else if observation.stage != from
                    && !allowed_lifecycle_transition(from, observation.stage)
                {
                    self.violations
                        .push(OracleViolation::InvalidLifecycleTransition {
                            rotation_id: observation.rotation_id,
                            from: Some(from),
                            to: observation.stage,
                        });
                } else {
                    self.lifecycle
                        .insert(observation.rotation_id, observation.stage);
                }
            } else if observation.stage != RotationLifecycleStage::Triggered {
                self.violations
                    .push(OracleViolation::InvalidLifecycleTransition {
                        rotation_id: observation.rotation_id,
                        from: None,
                        to: observation.stage,
                    });
            } else {
                self.lifecycle
                    .insert(observation.rotation_id, observation.stage);
            }
        } else if from.is_some_and(RotationLifecycleStage::terminal) {
            self.violations
                .push(OracleViolation::InvalidLifecycleTransition {
                    rotation_id: observation.rotation_id,
                    from,
                    to: observation.stage,
                });
        }
        self.lifecycle_events.push(observation);
    }

    fn require_owner(&mut self, session_id: Uuid, generation: Option<u64>, observation: &str) {
        let Some(generation) = generation else {
            self.violations.push(OracleViolation::MissingOwnership {
                session_id,
                observation: observation.to_owned(),
            });
            return;
        };
        match self.owner_states.get(&session_id).copied() {
            Some((active_generation, OwnershipState::Active))
                if active_generation == generation => {}
            Some((active_generation, OwnershipState::Active)) => {
                self.violations.push(OracleViolation::StaleGeneration {
                    session_id,
                    active_generation,
                    observed_generation: generation,
                    observation: observation.to_owned(),
                });
            }
            Some((_, state)) => {
                self.violations.push(OracleViolation::InactiveSession {
                    session_id,
                    state,
                    observation: observation.to_owned(),
                });
            }
            None => {
                self.violations.push(OracleViolation::MissingOwnership {
                    session_id,
                    observation: observation.to_owned(),
                });
            }
        }
    }
}

fn allowed_lifecycle_transition(from: RotationLifecycleStage, to: RotationLifecycleStage) -> bool {
    use RotationLifecycleStage as S;
    matches!(
        (from, to),
        (
            S::Triggered,
            S::IntakeFrozen
                | S::IntentPersisted
                | S::RecoveryPaused
                | S::Blocked
                | S::Cancelled
                | S::Failed
        ) | (
            S::IntakeFrozen,
            S::IntentPersisted | S::RecoveryPaused | S::Blocked | S::Cancelled | S::Failed
        ) | (
            S::IntentPersisted,
            S::SuccessorPrepared | S::RecoveryPaused | S::Blocked | S::Cancelled | S::Failed
        ) | (
            S::SuccessorPrepared,
            S::PointerPublished | S::RecoveryPaused | S::Blocked | S::Cancelled | S::Failed
        ) | (
            S::PointerPublished,
            S::PredecessorFenced | S::RecoveryPaused | S::Blocked | S::Cancelled | S::Failed
        ) | (
            S::PredecessorFenced,
            S::OwnershipCommitted | S::RecoveryPaused | S::Blocked | S::Cancelled | S::Failed
        ) | (
            S::OwnershipCommitted,
            S::SuccessorActivated | S::RecoveryPaused | S::Blocked | S::Cancelled | S::Failed
        ) | (
            S::RecoveryPaused,
            S::IntentPersisted
                | S::SuccessorPrepared
                | S::PointerPublished
                | S::PredecessorFenced
                | S::OwnershipCommitted
                | S::SuccessorActivated
                | S::Blocked
                | S::Cancelled
                | S::Failed
        )
    )
}

/// Deterministic fixtures shared by unit and integrated acceptance tests.
pub mod fixtures {
    use super::*;

    pub const FIXTURE_COMPACTION_FLOOR: u64 = 100;
    pub const FIXTURE_MAX_GROWTH: u64 = 10;
    pub const FIXTURE_MARGIN: u64 = 1;

    fn id(seed: u128) -> Uuid {
        Uuid::from_u128(seed)
    }

    pub fn fixture_session() -> Uuid {
        id(0xB055)
    }

    pub fn fixture_successor() -> Uuid {
        id(0xB055 + 1)
    }

    pub fn fixture_rotation() -> Uuid {
        id(0xB055 + 2)
    }

    pub fn fixture_runtime() -> Uuid {
        id(0xB055 + 3)
    }

    pub fn bounds(current_context: u64) -> ContextBounds {
        ContextBounds {
            compaction_floor: FIXTURE_COMPACTION_FLOOR,
            current_context,
            max_growth: FIXTURE_MAX_GROWTH,
            safety_margin: FIXTURE_MARGIN,
        }
    }

    pub fn ownership(
        session_id: Uuid,
        generation: u64,
        state: OwnershipState,
    ) -> RotationObservation {
        RotationObservation::Ownership(OwnershipObservation {
            session_id,
            generation,
            state,
        })
    }

    pub fn lifecycle(stage: RotationLifecycleStage) -> RotationObservation {
        RotationObservation::Lifecycle(LifecycleObservation {
            rotation_id: fixture_rotation(),
            old_session_id: fixture_session(),
            new_session_id: fixture_successor(),
            old_generation: 0,
            new_generation: 1,
            stage,
        })
    }

    pub fn admission(
        session_id: Uuid,
        owner_generation: u64,
        boundary: AdmissionBoundary,
        decision: AdmissionDecision,
    ) -> RotationObservation {
        RotationObservation::ProviderAdmission(ProviderAdmission {
            operation_id: format!("operation-{session_id}"),
            session_id,
            runtime_id: fixture_runtime(),
            owner_generation: Some(owner_generation),
            boundary,
            decision,
        })
    }

    pub fn boundary_trace(current_context: u64) -> Vec<RotationObservation> {
        vec![
            ownership(fixture_session(), 0, OwnershipState::Active),
            admission(
                fixture_session(),
                0,
                AdmissionBoundary::Request,
                AdmissionDecision::Admitted {
                    bounds: bounds(current_context),
                },
            ),
        ]
    }

    pub fn clean_rotation_trace() -> Vec<RotationObservation> {
        vec![
            ownership(fixture_session(), 0, OwnershipState::Active),
            lifecycle(RotationLifecycleStage::Triggered),
            lifecycle(RotationLifecycleStage::IntakeFrozen),
            lifecycle(RotationLifecycleStage::IntentPersisted),
            RotationObservation::DurableAction(DurableAction {
                action_id: "intent-1".into(),
                session_id: Some(fixture_session()),
                owner_generation: Some(0),
                kind: DurableActionKind::PersistedIntent,
                status: DurableActionStatus::Committed,
            }),
            lifecycle(RotationLifecycleStage::SuccessorPrepared),
            RotationObservation::DurableAction(DurableAction {
                action_id: "successor-1".into(),
                session_id: Some(fixture_successor()),
                owner_generation: Some(1),
                kind: DurableActionKind::SuccessorPreparation,
                status: DurableActionStatus::Committed,
            }),
            lifecycle(RotationLifecycleStage::PointerPublished),
            ownership(fixture_successor(), 1, OwnershipState::Published),
            lifecycle(RotationLifecycleStage::PredecessorFenced),
            ownership(fixture_session(), 0, OwnershipState::Fenced),
            lifecycle(RotationLifecycleStage::OwnershipCommitted),
            ownership(fixture_successor(), 1, OwnershipState::Active),
            lifecycle(RotationLifecycleStage::SuccessorActivated),
            admission(
                fixture_successor(),
                1,
                AdmissionBoundary::FirstRequest,
                AdmissionDecision::Admitted { bounds: bounds(10) },
            ),
        ]
    }

    /// Fault classes exercised by deterministic acceptance fixtures.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum FaultInjection {
        RejectedSend,
        LostAcknowledgement,
        CrashAfter(RotationLifecycleStage),
        DuplicateTrigger,
        ReplayedDispatch,
        UnsafeAdmission,
        ProviderCompaction,
    }

    pub fn fault_trace(fault: FaultInjection) -> Vec<RotationObservation> {
        let mut trace = clean_rotation_trace();
        match fault {
            FaultInjection::RejectedSend => trace.extend([
                transport("delivery-1", ProviderTransportStatus::Submitted),
                transport("delivery-1", ProviderTransportStatus::Rejected),
                transport("delivery-1", ProviderTransportStatus::Recovered),
                transport("delivery-1", ProviderTransportStatus::Submitted),
                transport("delivery-1", ProviderTransportStatus::Accepted),
            ]),
            FaultInjection::LostAcknowledgement => trace.extend([
                transport("delivery-2", ProviderTransportStatus::Submitted),
                transport("delivery-2", ProviderTransportStatus::Recovered),
                transport("delivery-2", ProviderTransportStatus::Submitted),
                transport("delivery-2", ProviderTransportStatus::Accepted),
                RotationObservation::DurableAction(DurableAction {
                    action_id: "effect-1".into(),
                    session_id: Some(fixture_successor()),
                    owner_generation: Some(1),
                    kind: DurableActionKind::ProviderEffect,
                    status: DurableActionStatus::Committed,
                }),
            ]),
            FaultInjection::CrashAfter(stage) => return crash_trace(stage),
            FaultInjection::DuplicateTrigger => {
                trace.insert(1, lifecycle(RotationLifecycleStage::Triggered));
            }
            FaultInjection::ReplayedDispatch => trace.extend([
                RotationObservation::DurableAction(DurableAction {
                    action_id: "dispatch-1".into(),
                    session_id: Some(fixture_successor()),
                    owner_generation: Some(1),
                    kind: DurableActionKind::ProviderDelivery,
                    status: DurableActionStatus::Submitted,
                }),
                RotationObservation::DurableAction(DurableAction {
                    action_id: "dispatch-1".into(),
                    session_id: Some(fixture_successor()),
                    owner_generation: Some(1),
                    kind: DurableActionKind::ProviderDelivery,
                    status: DurableActionStatus::Submitted,
                }),
                RotationObservation::DurableAction(DurableAction {
                    action_id: "dispatch-1".into(),
                    session_id: Some(fixture_successor()),
                    owner_generation: Some(1),
                    kind: DurableActionKind::ProviderEffect,
                    status: DurableActionStatus::Committed,
                }),
            ]),
            FaultInjection::UnsafeAdmission => trace.push(admission(
                fixture_successor(),
                1,
                AdmissionBoundary::Continuation,
                AdmissionDecision::Admitted {
                    bounds: ContextBounds {
                        current_context: FIXTURE_COMPACTION_FLOOR
                            - FIXTURE_MAX_GROWTH
                            - FIXTURE_MARGIN,
                        ..bounds(0)
                    },
                },
            )),
            FaultInjection::ProviderCompaction => trace.push(
                RotationObservation::ProviderCompaction(ProviderCompaction {
                    session_id: fixture_successor(),
                    runtime_id: fixture_runtime(),
                    owner_generation: Some(1),
                    stage: CompactionStage::Started,
                }),
            ),
        }
        trace
    }

    pub fn crash_trace(after: RotationLifecycleStage) -> Vec<RotationObservation> {
        let complete = clean_rotation_trace();
        let boundary = complete
            .iter()
            .position(|observation| {
                matches!(
                    observation,
                    RotationObservation::Lifecycle(lifecycle) if lifecycle.stage == after
                )
            })
            .expect("fixture lifecycle stage exists");
        let mut trace = complete[..=boundary].to_vec();
        trace.push(lifecycle(RotationLifecycleStage::Recovered));
        trace.extend(complete[boundary + 1..].iter().cloned());
        trace
    }

    fn transport(delivery_id: &str, status: ProviderTransportStatus) -> RotationObservation {
        RotationObservation::ProviderTransport(ProviderTransport {
            delivery_id: delivery_id.into(),
            session_id: fixture_successor(),
            runtime_id: fixture_runtime(),
            owner_generation: Some(1),
            boundary: AdmissionBoundary::Continuation,
            status,
        })
    }

    struct XorShift(u64);

    impl XorShift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// Sustained randomized ordering over one owned generation. Ownership is
    /// declared first; the independently emitted operation records vary after
    /// that durable boundary.
    pub fn randomized_trace(seed: u64, operations: usize) -> Vec<RotationObservation> {
        let mut random = XorShift(seed.max(1));
        let mut observations = vec![ownership(fixture_session(), 0, OwnershipState::Active)];
        for index in 0..operations {
            match random.next() % 4 {
                0 => observations.push(admission(
                    fixture_session(),
                    0,
                    AdmissionBoundary::Continuation,
                    AdmissionDecision::Admitted {
                        bounds: ContextBounds {
                            current_context: (index % 50) as u64,
                            max_growth: 1,
                            safety_margin: 1,
                            compaction_floor: FIXTURE_COMPACTION_FLOOR,
                        },
                    },
                )),
                1 => observations.push(RotationObservation::ProviderTransport(ProviderTransport {
                    delivery_id: format!("delivery-{index}"),
                    session_id: fixture_session(),
                    runtime_id: fixture_runtime(),
                    owner_generation: Some(0),
                    boundary: AdmissionBoundary::Continuation,
                    status: ProviderTransportStatus::Accepted,
                })),
                2 => observations.push(RotationObservation::DurableAction(DurableAction {
                    action_id: format!("effect-{index}"),
                    session_id: Some(fixture_session()),
                    owner_generation: Some(0),
                    kind: DurableActionKind::ProviderEffect,
                    status: DurableActionStatus::Committed,
                })),
                _ => observations.push(RotationObservation::ContextSample(ContextSample {
                    session_id: fixture_session(),
                    runtime_id: fixture_runtime(),
                    owner_generation: Some(0),
                    context_tokens: index as u64,
                    context_window: Some(FIXTURE_COMPACTION_FLOOR),
                    source: ContextSampleSource::ProviderReport,
                })),
            }
        }
        observations
    }

    pub fn shuffled_tail(
        mut observations: Vec<RotationObservation>,
        seed: u64,
    ) -> Vec<RotationObservation> {
        let mut random = XorShift(seed.max(1));
        for index in (2..observations.len()).rev() {
            let swap = 1 + (random.next() as usize % index);
            observations.swap(index, swap);
        }
        observations
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn boundary_fixture_enforces_strict_safety_inequality() {
        // C=100, G=10, M=1: U=88 leaves one unit, U=89 is equality, U=90 overflows.
        for (current_context, clean) in [(88, true), (89, false), (90, false)] {
            let report = RotationOracle::evaluate(boundary_trace(current_context));
            assert_eq!(report.is_clean(), clean, "U={current_context}");
            assert_eq!(report.unsafe_admissions, usize::from(!clean));
            if clean {
                assert_eq!(report.min_headroom, Some(1));
            }
        }
    }

    #[test]
    fn oracle_detects_a_deliberately_unguarded_or_unsafe_admission() {
        let mut trace = boundary_trace(20);
        trace.push(admission(
            fixture_session(),
            0,
            AdmissionBoundary::Continuation,
            AdmissionDecision::Unguarded,
        ));
        trace.push(admission(
            fixture_session(),
            0,
            AdmissionBoundary::Continuation,
            AdmissionDecision::Admitted { bounds: bounds(89) },
        ));
        let report = RotationOracle::evaluate(trace);
        assert_eq!(report.unsafe_admissions, 2);
        assert!(report.violations.iter().any(|violation| {
            matches!(violation, OracleViolation::MissingAdmissionGuard { .. })
        }));
        assert!(
            report
                .violations
                .iter()
                .any(|violation| { matches!(violation, OracleViolation::UnsafeAdmission { .. }) })
        );
    }

    #[test]
    fn fault_fixtures_cover_rejected_lost_ack_crashes_duplicates_and_replay() {
        for fault in [
            FaultInjection::RejectedSend,
            FaultInjection::LostAcknowledgement,
            FaultInjection::DuplicateTrigger,
            FaultInjection::ReplayedDispatch,
        ] {
            let report = RotationOracle::evaluate(fault_trace(fault));
            assert!(report.is_clean(), "{fault:?}: {:?}", report.violations);
            assert_eq!(report.completed_rotations, 1);
        }
        for stage in [
            RotationLifecycleStage::Triggered,
            RotationLifecycleStage::IntakeFrozen,
            RotationLifecycleStage::IntentPersisted,
            RotationLifecycleStage::SuccessorPrepared,
            RotationLifecycleStage::PointerPublished,
            RotationLifecycleStage::PredecessorFenced,
            RotationLifecycleStage::OwnershipCommitted,
        ] {
            let report = RotationOracle::evaluate(fault_trace(FaultInjection::CrashAfter(stage)));
            assert!(report.is_clean(), "{stage:?}: {:?}", report.violations);
            assert_eq!(report.completed_rotations, 1);
        }
        let unsafe_report = RotationOracle::evaluate(fault_trace(FaultInjection::UnsafeAdmission));
        assert_eq!(unsafe_report.unsafe_admissions, 1);
        let compacted = RotationOracle::evaluate(fault_trace(FaultInjection::ProviderCompaction));
        assert_eq!(compacted.compactions, 1);
        assert!(
            compacted.violations.iter().any(|violation| {
                matches!(violation, OracleViolation::CompactionEntered { .. })
            })
        );
    }

    #[test]
    fn replay_cannot_commit_two_effects_for_one_action() {
        let mut trace = clean_rotation_trace();
        for _ in 0..2 {
            trace.push(RotationObservation::DurableAction(DurableAction {
                action_id: "effect-replay".into(),
                session_id: Some(fixture_successor()),
                owner_generation: Some(1),
                kind: DurableActionKind::ProviderEffect,
                status: DurableActionStatus::Committed,
            }));
        }
        let report = RotationOracle::evaluate(trace);
        assert_eq!(report.provider_effects, 1);
        assert!(report.violations.iter().any(|violation| {
            matches!(violation, OracleViolation::DuplicateProviderEffect { .. })
        }));
    }

    #[test]
    fn missing_compaction_telemetry_is_explicit_unless_the_contract_verifies_disable() {
        let observation =
            RotationObservation::CompactionTelemetry(CompactionTelemetryObservation {
                session_id: fixture_session(),
                owner_generation: Some(0),
                telemetry: CompactionTelemetry::Unavailable,
            });
        let unqualified = RotationOracle::evaluate([observation.clone()]);
        assert!(unqualified.violations.iter().any(|violation| {
            matches!(
                violation,
                OracleViolation::CompactionTelemetryUnavailable { .. }
            )
        }));
        let qualified = RotationOracle::evaluate_with(
            ProviderQualification {
                verified_no_compaction: true,
            },
            [observation],
        );
        assert!(qualified.is_clean());
    }

    #[test]
    fn randomized_event_ordering_is_deterministic_and_still_detects_unsafe_work() {
        let first = randomized_trace(0x5EED, 256);
        let second = randomized_trace(0x5EED, 256);
        assert_eq!(first, second);
        let report = RotationOracle::evaluate(first.clone());
        assert!(report.is_clean(), "{:?}", report.violations);

        let mut unsafe_trace = first;
        unsafe_trace.push(admission(
            fixture_session(),
            0,
            AdmissionBoundary::Continuation,
            AdmissionDecision::Admitted { bounds: bounds(95) },
        ));
        for seed in [1, 2, 3, 4, 5] {
            let report = RotationOracle::evaluate(shuffled_tail(unsafe_trace.clone(), seed));
            assert_eq!(report.unsafe_admissions, 1);
        }
    }
}
