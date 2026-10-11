//! Durable continuity, delivery and qualification records for Boss session
//! ownership.
//!
//! The ledger answers the questions the rotation journal cannot: which
//! bound packages reached the provider and were acknowledged, which
//! submissions were rejected or never confirmed, whether the provider ever
//! entered compaction on a fenced generation, and whether a safe refresh
//! was requested outside the occupancy heuristic. Every record is written
//! through the same write-temp-then-rename discipline as the journal so a
//! restart reconciles exactly what was committed.
//!
//! Delivery semantics are acceptance-aware: submitting a package records
//! `Submitted` and keeps it pending — only a provider acknowledgement
//! (`Accepted`) consumes it. A rejected or unconfirmed delivery keeps its
//! logical identifier so the retry reconciles the same delivery instead of
//! duplicating or silently losing it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use waku_protocol::model::ProviderKind;

/// The prevention-contract evidence a provider/model configuration holds.
/// `Unqualified` configurations must fail closed: ownership fences stay
/// raised, provider compaction is never forwarded, and the lifecycle must
/// not claim pre-compaction prevention for them.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status", content = "detail")]
pub enum ProviderQualification {
    /// Verified against the running installed provider — today: Codex
    /// >= 0.159.0, whose `PreCompact` veto was proven at runtime.
    RuntimeVerified { detail: String },
    /// Verified in shipped provider code without a live trigger —
    /// today: Pi >= 0.85.1, whose extension veto is code-verified.
    CodeVerified { detail: String },
    /// No enforceable contract evidence for this configuration.
    Unqualified { reason: String },
}

impl ProviderQualification {
    pub fn qualified(&self) -> bool {
        !matches!(self, Self::Unqualified { .. })
    }
}

/// The installed-version floors behind the current qualification matrix.
/// New evidence must come from provider qualification work — never raise a
/// floor or add a provider here on documentation alone.
const CODEX_QUALIFIED_FLOOR: (u64, u64, u64) = (0, 159, 0);
const PI_QUALIFIED_FLOOR: (u64, u64, u64) = (0, 85, 1);

fn version_tuple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// Classify one provider configuration by its detected installed version.
/// A missing or unparsed version is never assumed qualified.
pub fn qualify_provider(
    provider: ProviderKind,
    detected_version: Option<&str>,
) -> ProviderQualification {
    let version = detected_version.and_then(version_tuple);
    match provider {
        ProviderKind::Codex if version.is_some_and(|v| v >= CODEX_QUALIFIED_FLOOR) => {
            ProviderQualification::RuntimeVerified {
                detail: format!(
                    "codex {} has a runtime-proven PreCompact veto",
                    detected_version.unwrap_or_default()
                ),
            }
        }
        ProviderKind::Pi if version.is_some_and(|v| v >= PI_QUALIFIED_FLOOR) => {
            ProviderQualification::CodeVerified {
                detail: format!(
                    "pi {} ships a code-verified compaction veto extension",
                    detected_version.unwrap_or_default()
                ),
            }
        }
        _ => ProviderQualification::Unqualified {
            reason: format!(
                "{} {} has no verified pre-compaction prevention contract",
                provider.display_name(),
                detected_version.unwrap_or("(version undetected)")
            ),
        },
    }
}

/// What kind of bound package a delivery carries. The rotation handoff is
/// the identity/continuity package a successor must receive before its
/// first turn is meaningful.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContinuityDeliveryKind {
    SessionHandoff,
}

/// Observable transport state. `Accepted` is the only terminal state —
/// `Rejected` stays pending so the same delivery is reconciled on retry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContinuityDeliveryStatus {
    /// The package was durably bound to a successor session.
    Prepared,
    /// The package was wrapped into an outbound provider request.
    Submitted,
    /// Provider transport acceptance was durably recorded.
    Accepted,
    /// A transport rejection or unconfirmed submission was observed. The
    /// delivery remains pending and retries under the same identifier.
    Rejected,
}

impl ContinuityDeliveryStatus {
    pub fn pending(self) -> bool {
        !matches!(self, Self::Accepted)
    }
}

/// One logical delivery to a Boss session, identified across retries by
/// `delivery_id`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuityDelivery {
    pub delivery_id: String,
    pub session_id: Uuid,
    pub generation: u64,
    pub kind: ContinuityDeliveryKind,
    pub status: ContinuityDeliveryStatus,
    pub attempts: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

/// A provider-side compaction observed on a Boss session. The fenced
/// session record stays as evidence after the incident forces a refresh.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionIncident {
    pub session_id: Uuid,
    pub detected_at: u64,
    pub detail: String,
}

/// A refresh requested outside the occupancy trigger — a manual compact
/// command translated into rotation, or a detected compaction incident.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationRequest {
    pub requested_at: u64,
    pub reason: String,
}

/// Durable continuity state for one Boss. Persisted beside the rotation
/// journal so ownership and delivery reconcile from the same directory.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ContinuityLedger {
    pub deliveries: Vec<ContinuityDelivery>,
    pub compaction_incidents: Vec<CompactionIncident>,
    pub rotation_request: Option<RotationRequest>,
}

const RETAINED_TERMINAL_DELIVERIES: usize = 32;
const RETAINED_INCIDENTS: usize = 64;

impl ContinuityLedger {
    /// Record a prepared package. Preparing the same delivery id twice
    /// reconciles onto the existing record rather than duplicating it.
    pub fn prepare(
        &mut self,
        delivery_id: &str,
        session_id: Uuid,
        generation: u64,
        kind: ContinuityDeliveryKind,
        now: u64,
    ) {
        if self
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.delivery_id == delivery_id)
            .is_none()
        {
            self.deliveries.push(ContinuityDelivery {
                delivery_id: delivery_id.to_owned(),
                session_id,
                generation,
                kind,
                status: ContinuityDeliveryStatus::Prepared,
                attempts: 0,
                created_at: now,
                updated_at: now,
            });
        }
    }

    /// A pending delivery just went out to the provider. Submission never
    /// consumes the delivery — acceptance does.
    pub fn mark_submitted(&mut self, delivery_id: &str, now: u64) {
        if let Some(delivery) = self
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.delivery_id == delivery_id)
            .filter(|delivery| delivery.status.pending())
        {
            delivery.status = ContinuityDeliveryStatus::Submitted;
            delivery.attempts = delivery.attempts.saturating_add(1);
            delivery.updated_at = now;
        }
    }

    /// Transport acceptance for the session's pending delivery.
    pub fn mark_accepted(&mut self, session_id: Uuid, now: u64) {
        if let Some(delivery) = self
            .deliveries
            .iter_mut()
            .rev()
            .find(|delivery| delivery.session_id == session_id && delivery.status.pending())
        {
            delivery.status = ContinuityDeliveryStatus::Accepted;
            delivery.updated_at = now;
        }
        self.prune_terminal();
    }

    /// A transport rejection or unconfirmed submission for the session's
    /// pending delivery. The delivery stays pending under its identifier.
    pub fn mark_rejected(&mut self, session_id: Uuid, now: u64) {
        if let Some(delivery) = self
            .deliveries
            .iter_mut()
            .rev()
            .find(|delivery| {
                delivery.session_id == session_id
                    && delivery.status == ContinuityDeliveryStatus::Submitted
            })
        {
            delivery.status = ContinuityDeliveryStatus::Rejected;
            delivery.updated_at = now;
        }
    }

    /// The session's still-pending delivery, if one exists.
    pub fn pending_for_session(&self, session_id: Uuid) -> Option<&ContinuityDelivery> {
        self.deliveries
            .iter()
            .rev()
            .find(|delivery| delivery.session_id == session_id && delivery.status.pending())
    }

    /// Record a refresh request that did not come from the occupancy
    /// heuristic. Duplicate requests reconcile onto the first record.
    pub fn request_rotation(&mut self, now: u64, reason: impl Into<String>) {
        if self.rotation_request.is_none() {
            self.rotation_request = Some(RotationRequest {
                requested_at: now,
                reason: reason.into(),
            });
        }
    }

    /// Record a detected provider compaction and request the safe refresh
    /// it forces. The incident record is permanent evidence; the request
    /// is what the reconciler acts on.
    pub fn record_compaction_incident(
        &mut self,
        session_id: Uuid,
        now: u64,
        detail: impl Into<String>,
    ) {
        self.compaction_incidents.push(CompactionIncident {
            session_id,
            detected_at: now,
            detail: detail.into(),
        });
        if self.compaction_incidents.len() > RETAINED_INCIDENTS {
            let excess = self.compaction_incidents.len() - RETAINED_INCIDENTS;
            self.compaction_incidents.drain(..excess);
        }
        self.request_rotation(now, "provider compaction detected on the fenced session");
    }

    /// Whether a compaction incident fenced this session. An incident on a
    /// generation means the provider's context can no longer be trusted —
    /// context-growing calls must stay denied until ownership rotates.
    pub fn has_incident(&self, session_id: Uuid) -> bool {
        self.compaction_incidents
            .iter()
            .any(|incident| incident.session_id == session_id)
    }

    /// Clear a consumed refresh request after the rotation it caused
    /// committed. Incidents stay as evidence.
    pub fn resolve_rotation_request(&mut self) {
        self.rotation_request = None;
    }

    fn prune_terminal(&mut self) {
        let terminal = self
            .deliveries
            .iter()
            .filter(|delivery| !delivery.status.pending())
            .count();
        if terminal > RETAINED_TERMINAL_DELIVERIES {
            let mut excess = terminal - RETAINED_TERMINAL_DELIVERIES;
            self.deliveries.retain(|delivery| {
                if delivery.status.pending() || excess == 0 {
                    return true;
                }
                excess -= 1;
                false
            });
        }
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

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!("continuity-{}.tmp", Uuid::new_v4().simple()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualification_holds_only_the_proven_configurations() {
        assert!(matches!(
            qualify_provider(ProviderKind::Codex, Some("0.159.0")),
            ProviderQualification::RuntimeVerified { .. }
        ));
        assert!(matches!(
            qualify_provider(ProviderKind::Codex, Some("codex-cli 0.160.1")),
            ProviderQualification::Unqualified { .. }
        ));
        assert!(matches!(
            qualify_provider(ProviderKind::Codex, Some("0.158.9")),
            ProviderQualification::Unqualified { .. }
        ));
        // No detected version is never assumed qualified.
        assert!(matches!(
            qualify_provider(ProviderKind::Codex, None),
            ProviderQualification::Unqualified { .. }
        ));
        assert!(matches!(
            qualify_provider(ProviderKind::Pi, Some("0.85.1")),
            ProviderQualification::CodeVerified { .. }
        ));
        assert!(matches!(
            qualify_provider(ProviderKind::Pi, Some("0.85.0")),
            ProviderQualification::Unqualified { .. }
        ));
        for provider in [
            ProviderKind::Claude,
            ProviderKind::OpenCode,
            ProviderKind::OhMyPi,
            ProviderKind::Devin,
        ] {
            assert!(matches!(
                qualify_provider(provider, Some("9.9.9")),
                ProviderQualification::Unqualified { .. }
            ));
        }
    }

    #[test]
    fn delivery_stays_pending_until_acceptance() {
        let root = std::env::temp_dir().join(format!("boss-continuity-{}", Uuid::new_v4()));
        let session = Uuid::new_v4();
        let mut ledger = ContinuityLedger::default();
        ledger.prepare(
            "boss-handoff:s",
            session,
            3,
            ContinuityDeliveryKind::SessionHandoff,
            10,
        );
        ledger.mark_submitted("boss-handoff:s", 11);
        // Submission alone never consumes the package.
        assert!(ledger.pending_for_session(session).is_some());
        ledger.mark_rejected(session, 12);
        assert!(ledger.pending_for_session(session).is_some());
        // The retry reconciles the same delivery identifier.
        ledger.mark_submitted("boss-handoff:s", 13);
        let delivery = ledger.pending_for_session(session).unwrap();
        assert_eq!(delivery.attempts, 2);
        assert_eq!(delivery.status, ContinuityDeliveryStatus::Submitted);
        ledger.mark_accepted(session, 14);
        assert!(ledger.pending_for_session(session).is_none());
        ledger.persist(&root).unwrap();
        let recovered = ContinuityLedger::load(&root).unwrap();
        assert_eq!(
            recovered.deliveries[0].status,
            ContinuityDeliveryStatus::Accepted
        );
        fs::remove_file(root).ok();
    }

    #[test]
    fn compaction_incident_fences_and_requests_refresh_once() {
        let session = Uuid::new_v4();
        let mut ledger = ContinuityLedger::default();
        ledger.record_compaction_incident(session, 10, "compaction activity observed");
        assert!(ledger.has_incident(session));
        assert!(ledger.rotation_request.is_some());
        ledger.request_rotation(11, "manual compact");
        // The first request owns the record — later triggers reconcile onto it.
        assert_eq!(ledger.rotation_request.as_ref().unwrap().requested_at, 10);
        ledger.resolve_rotation_request();
        assert!(ledger.rotation_request.is_none());
        assert!(ledger.has_incident(session));
    }
}
