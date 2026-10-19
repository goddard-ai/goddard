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
use sha2::{Digest, Sha256};
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

/// Exact provider-facing continuity content. The adapter must deliver `instructions`
/// through its highest-priority persistent instruction surface and `context`
/// intact. Neither a summary nor a model acknowledgement is a receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuityPackage {
    pub identity_revision: String,
    pub scope: ContinuityScope,
    pub instructions: String,
    pub context: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContinuityScope {
    Coordinator,
    Planning,
}

impl ContinuityPackage {
    pub fn digest(&self) -> anyhow::Result<String> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
}

/// Correlation an adapter must echo in its acceptance or definitive rejection
/// receipt. This value is prepared before submission and alone proves no
/// acceptance. Runtime and attempt identity prevent delayed events from
/// accepting a retry or a replacement process.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuityAttempt {
    pub delivery_id: String,
    pub session_id: Uuid,
    pub generation: u64,
    pub runtime_id: Uuid,
    pub attempt_id: Uuid,
    pub package_digest: String,
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
    /// Absent for legacy session-only deliveries. Never upgrade those
    /// historical acceptances into proof of exact-package receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<ContinuityPackage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<ContinuityAttempt>,
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
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
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
                package: None,
                attempt: None,
            });
        }
    }

    /// Prepare an immutable exact package. Reusing a logical identity with
    /// different content, scope, session or generation is an integrity error.
    /// Legacy records cannot acquire stronger evidence after the fact.
    pub fn prepare_package(
        &mut self,
        delivery_id: &str,
        session_id: Uuid,
        generation: u64,
        package: ContinuityPackage,
        now: u64,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !package.identity_revision.is_empty(),
            "missing continuity identity revision"
        );
        anyhow::ensure!(
            !package.instructions.is_empty(),
            "missing canonical continuity instructions"
        );
        if let Some(existing) = self
            .deliveries
            .iter()
            .find(|item| item.delivery_id == delivery_id)
        {
            anyhow::ensure!(
                existing.session_id == session_id
                    && existing.generation == generation
                    && existing.kind == ContinuityDeliveryKind::SessionHandoff
                    && existing.package.as_ref() == Some(&package),
                "continuity delivery binding conflict"
            );
            return Ok(());
        }
        self.prepare(
            delivery_id,
            session_id,
            generation,
            ContinuityDeliveryKind::SessionHandoff,
            now,
        );
        self.deliveries
            .last_mut()
            .expect("prepared delivery exists")
            .package = Some(package);
        Ok(())
    }

    /// Persist this returned correlation before contacting the adapter, using
    /// `update`. A submitted attempt has unknown effects and cannot be retried
    /// automatically: reconcile its receipt or explicitly record rejection.
    pub fn submit_package(
        &mut self,
        delivery_id: &str,
        runtime_id: Uuid,
        now: u64,
    ) -> anyhow::Result<ContinuityAttempt> {
        let delivery = self
            .deliveries
            .iter_mut()
            .find(|item| item.delivery_id == delivery_id)
            .ok_or_else(|| anyhow::anyhow!("continuity delivery is missing"))?;
        anyhow::ensure!(
            matches!(
                delivery.status,
                ContinuityDeliveryStatus::Prepared | ContinuityDeliveryStatus::Rejected
            ),
            "continuity submission requires reconciliation"
        );
        let package = delivery
            .package
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("legacy continuity delivery has no exact package"))?;
        let receipt = ContinuityAttempt {
            delivery_id: delivery.delivery_id.clone(),
            session_id: delivery.session_id,
            generation: delivery.generation,
            runtime_id,
            attempt_id: Uuid::new_v4(),
            package_digest: package.digest()?,
        };
        delivery.status = ContinuityDeliveryStatus::Submitted;
        delivery.attempts = delivery.attempts.saturating_add(1);
        delivery.updated_at = now;
        delivery.attempt = Some(receipt.clone());
        Ok(receipt)
    }

    /// Apply adapter receipt evidence, never an unbound turn-start event.
    /// Duplicate receipts reconcile successfully; foreign or stale receipts
    /// leave the pending package unchanged.
    pub fn accept_package(&mut self, receipt: &ContinuityAttempt, now: u64) -> bool {
        let Some(delivery) = self
            .deliveries
            .iter_mut()
            .find(|item| item.delivery_id == receipt.delivery_id)
        else {
            return false;
        };
        if delivery.attempt.as_ref() != Some(receipt)
            || delivery.session_id != receipt.session_id
            || delivery.generation != receipt.generation
            || !delivery.package.as_ref().is_some_and(|package| {
                package.digest().ok().as_ref() == Some(&receipt.package_digest)
            })
            || !matches!(
                delivery.status,
                ContinuityDeliveryStatus::Submitted | ContinuityDeliveryStatus::Accepted
            )
        {
            return false;
        }
        if delivery.status == ContinuityDeliveryStatus::Accepted {
            return true;
        }
        delivery.status = ContinuityDeliveryStatus::Accepted;
        delivery.updated_at = now;
        self.prune_terminal();
        true
    }

    /// Only a correlated definitive adapter rejection permits another
    /// attempt. A process exit or lost acknowledgement is not rejection.
    pub fn reject_package(&mut self, receipt: &ContinuityAttempt, now: u64) -> bool {
        let Some(delivery) = self
            .deliveries
            .iter_mut()
            .find(|item| item.delivery_id == receipt.delivery_id)
        else {
            return false;
        };
        if delivery.status != ContinuityDeliveryStatus::Submitted
            || delivery.attempt.as_ref() != Some(receipt)
        {
            return false;
        }
        delivery.status = ContinuityDeliveryStatus::Rejected;
        delivery.updated_at = now;
        true
    }

    /// Record legacy submission. Exact packages require `submit_package`
    /// and cannot be submitted through this session-only compatibility path.
    pub fn mark_submitted(&mut self, delivery_id: &str, now: u64) {
        if let Some(delivery) = self
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.delivery_id == delivery_id)
            .filter(|delivery| delivery.status.pending() && delivery.package.is_none())
        {
            delivery.status = ContinuityDeliveryStatus::Submitted;
            delivery.attempts = delivery.attempts.saturating_add(1);
            delivery.updated_at = now;
        }
    }

    /// Legacy transport acceptance can settle only a submitted legacy
    /// delivery. Exact packages require `accept_package`; a session-only
    /// event never acknowledges them. Return whether acceptance was recorded.
    pub fn mark_accepted(&mut self, session_id: Uuid, now: u64) -> bool {
        let Some(delivery) = self
            .deliveries
            .iter_mut()
            .rev()
            .find(|delivery| delivery.session_id == session_id && delivery.status.pending())
        else {
            return false;
        };
        if delivery.status != ContinuityDeliveryStatus::Submitted || delivery.package.is_some() {
            return false;
        }
        delivery.status = ContinuityDeliveryStatus::Accepted;
        delivery.updated_at = now;
        self.prune_terminal();
        true
    }

    /// Legacy rejection/unconfirmed submission. Exact packages remain
    /// unresolved until a correlated definitive `reject_package` receipt.
    pub fn mark_rejected(&mut self, session_id: Uuid, now: u64) {
        if let Some(delivery) = self.deliveries.iter_mut().rev().find(|delivery| {
            delivery.session_id == session_id
                && delivery.status == ContinuityDeliveryStatus::Submitted
                && delivery.package.is_none()
        }) {
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
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        use std::io::Write;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(temp, path)?;
        Ok(())
    }

    /// Serialize read/modify/write across event pumps, rotation and manual
    /// requests. Return the result only after its changes are committed.
    /// The lock file survives rename of the ledger. Callers must not hold
    /// this lock while acquiring task or Boss state leases.
    pub fn update<T>(path: &Path, change: impl FnOnce(&mut Self) -> T) -> anyhow::Result<T> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))?;
        lock.lock()?;
        let mut ledger = Self::load(path)?;
        let before = ledger.clone();
        let result = change(&mut ledger);
        if ledger != before {
            ledger.persist(path)?;
        }
        Ok(result)
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => {
                let ledger: Self = serde_json::from_slice(&bytes)?;
                for delivery in &ledger.deliveries {
                    let Some(package) = &delivery.package else {
                        anyhow::ensure!(
                            delivery.attempt.is_none(),
                            "continuity attempt has no package"
                        );
                        continue;
                    };
                    anyhow::ensure!(
                        !package.identity_revision.is_empty() && !package.instructions.is_empty(),
                        "continuity package identity is missing"
                    );
                    if let Some(attempt) = &delivery.attempt {
                        anyhow::ensure!(
                            delivery.status != ContinuityDeliveryStatus::Prepared,
                            "continuity prepared package has a submitted attempt"
                        );
                        anyhow::ensure!(
                            attempt.delivery_id == delivery.delivery_id
                                && attempt.session_id == delivery.session_id
                                && attempt.generation == delivery.generation
                                && attempt.package_digest == package.digest()?,
                            "continuity package binding is invalid"
                        );
                    } else {
                        anyhow::ensure!(
                            delivery.status == ContinuityDeliveryStatus::Prepared,
                            "continuity package receipt correlation is missing"
                        );
                    }
                }
                Ok(ledger)
            }
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

    fn package() -> ContinuityPackage {
        ContinuityPackage {
            identity_revision: "policy-revision-7".into(),
            scope: ContinuityScope::Planning,
            instructions: "Coordinate design only; no implementation or finalization grant.".into(),
            context: "Employee result pending; approval remains unresolved.".into(),
        }
    }

    #[test]
    fn exact_package_rejects_rebinding_and_legacy_receipts() {
        let session = Uuid::new_v4();
        let mut ledger = ContinuityLedger::default();
        ledger
            .prepare_package("bound", session, 7, package(), 10)
            .unwrap();
        let prepared = ledger.clone();
        for changed in [
            ContinuityPackage {
                scope: ContinuityScope::Coordinator,
                ..package()
            },
            ContinuityPackage {
                identity_revision: "changed".into(),
                ..package()
            },
            ContinuityPackage {
                instructions: "different instructions".into(),
                ..package()
            },
            ContinuityPackage {
                context: "different result".into(),
                ..package()
            },
        ] {
            assert!(
                ledger
                    .prepare_package("bound", session, 7, changed, 11)
                    .is_err()
            );
            assert_eq!(ledger, prepared);
        }
        assert!(
            ledger
                .prepare_package("bound", Uuid::new_v4(), 7, package(), 11)
                .is_err()
        );
        assert!(
            ledger
                .prepare_package("bound", session, 8, package(), 11)
                .is_err()
        );
        ledger
            .prepare_package("bound", session, 7, package(), 11)
            .unwrap();
        assert_eq!(ledger, prepared);
        ledger.mark_submitted("bound", 12);
        assert_eq!(ledger, prepared);
        let receipt = ledger.submit_package("bound", Uuid::new_v4(), 12).unwrap();
        assert!(!ledger.mark_accepted(session, 13));
        ledger.mark_rejected(session, 13);
        assert_eq!(
            ledger.deliveries[0].status,
            ContinuityDeliveryStatus::Submitted
        );
        assert!(ledger.accept_package(&receipt, 14));
        assert!(ledger.accept_package(&receipt, 15));
        assert_eq!(ledger.deliveries[0].updated_at, 14);

        ledger.prepare(
            "legacy",
            session,
            8,
            ContinuityDeliveryKind::SessionHandoff,
            16,
        );
        assert!(
            ledger
                .prepare_package("legacy", session, 8, package(), 17)
                .is_err()
        );
        assert!(ledger.submit_package("legacy", Uuid::new_v4(), 17).is_err());
    }

    #[test]
    fn exact_package_receipt_reconciles_unknown_effect_after_restart() {
        let root = std::env::temp_dir().join(format!("boss-exact-{}", Uuid::new_v4()));
        let path = root.join("continuity.json");
        let session = Uuid::new_v4();
        let runtime = Uuid::new_v4();
        let receipt = ContinuityLedger::update(&path, |ledger| {
            ledger.prepare_package("bound", session, 7, package(), 10)?;
            ledger.submit_package("bound", runtime, 11)
        })
        .unwrap()
        .unwrap();
        let mut reopened = ContinuityLedger::load(&path).unwrap();
        assert_eq!(reopened.deliveries[0].package, Some(package()));
        // Losing the response cannot turn unknown effects into permission to replay.
        assert!(reopened.submit_package("bound", runtime, 12).is_err());
        let before = reopened.clone();
        let mut foreign = Vec::new();
        let mut changed = receipt.clone();
        changed.delivery_id = "other".into();
        foreign.push(changed);
        let mut changed = receipt.clone();
        changed.session_id = Uuid::new_v4();
        foreign.push(changed);
        let mut changed = receipt.clone();
        changed.generation += 1;
        foreign.push(changed);
        let mut changed = receipt.clone();
        changed.runtime_id = Uuid::new_v4();
        foreign.push(changed);
        let mut changed = receipt.clone();
        changed.attempt_id = Uuid::new_v4();
        foreign.push(changed);
        let mut changed = receipt.clone();
        changed.package_digest = "wrong".into();
        foreign.push(changed);
        for forged in &foreign {
            assert!(!reopened.accept_package(forged, 12));
            assert!(!reopened.reject_package(forged, 12));
            assert_eq!(reopened, before);
        }
        assert!(reopened.reject_package(&receipt, 13));
        assert!(!reopened.accept_package(&receipt, 14));
        let retry = reopened.submit_package("bound", runtime, 15).unwrap();
        assert_ne!(receipt.attempt_id, retry.attempt_id);
        assert!(!reopened.accept_package(&receipt, 16));
        reopened.persist(&path).unwrap();
        assert!(
            ContinuityLedger::update(&path, |ledger| ledger.accept_package(&retry, 17)).unwrap()
        );
        let accepted = ContinuityLedger::load(&path).unwrap();
        assert!(accepted.pending_for_session(session).is_none());
        assert_eq!(accepted.deliveries[0].attempts, 2);
        assert_eq!(accepted.deliveries[0].package, Some(package()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_package_corruption_cannot_recover_as_accepted() {
        let root = std::env::temp_dir().join(format!("boss-exact-integrity-{}", Uuid::new_v4()));
        let path = root.join("continuity.json");
        let mut ledger = ContinuityLedger::default();
        ledger
            .prepare_package("bound", Uuid::new_v4(), 1, package(), 10)
            .unwrap();
        let receipt = ledger.submit_package("bound", Uuid::new_v4(), 11).unwrap();
        assert!(ledger.accept_package(&receipt, 12));
        ledger.persist(&path).unwrap();
        assert!(ContinuityLedger::load(&path).is_ok());
        let accepted = ledger.clone();
        ledger.deliveries[0].package.as_mut().unwrap().scope = ContinuityScope::Coordinator;
        ledger.persist(&path).unwrap();
        assert!(ContinuityLedger::load(&path).is_err());
        ledger = accepted;
        ledger.deliveries[0].attempt = None;
        ledger.persist(&path).unwrap();
        assert!(ContinuityLedger::load(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_package_acceptance_write_failure_keeps_unknown_attempt() {
        let root = std::env::temp_dir().join(format!("boss-exact-fault-{}", Uuid::new_v4()));
        let path = root.join("continuity.json");
        let receipt = ContinuityLedger::update(&path, |ledger| {
            ledger.prepare_package("bound", Uuid::new_v4(), 1, package(), 10)?;
            ledger.submit_package("bound", Uuid::new_v4(), 11)
        })
        .unwrap()
        .unwrap();
        let retained = root.join("retained.json");
        let result = ContinuityLedger::update(&path, |ledger| {
            fs::rename(&path, &retained).unwrap();
            fs::create_dir(&path).unwrap();
            ledger.accept_package(&receipt, 12)
        });
        assert!(result.is_err());
        fs::remove_dir(&path).unwrap();
        fs::rename(&retained, &path).unwrap();
        let mut reopened = ContinuityLedger::load(&path).unwrap();
        assert_eq!(
            reopened.deliveries[0].status,
            ContinuityDeliveryStatus::Submitted
        );
        assert!(
            reopened
                .submit_package("bound", Uuid::new_v4(), 13)
                .is_err()
        );
        assert!(
            ContinuityLedger::update(&path, |ledger| ledger.accept_package(&receipt, 14)).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

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
        assert!(!ledger.mark_accepted(session, 10));
        assert_eq!(
            ledger.pending_for_session(session).unwrap().status,
            ContinuityDeliveryStatus::Prepared
        );
        ledger.mark_submitted("boss-handoff:s", 11);
        // Submission alone never consumes the package.
        assert!(ledger.pending_for_session(session).is_some());
        ledger.mark_rejected(session, 12);
        assert!(ledger.pending_for_session(session).is_some());
        assert!(!ledger.mark_accepted(session, 12));
        // The retry reconciles the same delivery identifier.
        ledger.mark_submitted("boss-handoff:s", 13);
        let delivery = ledger.pending_for_session(session).unwrap();
        assert_eq!(delivery.attempts, 2);
        assert_eq!(delivery.status, ContinuityDeliveryStatus::Submitted);
        ledger.persist(&root).unwrap();
        assert!(
            ContinuityLedger::update(&root, |ledger| ledger.mark_accepted(session, 14)).unwrap()
        );
        let recovered = ContinuityLedger::load(&root).unwrap();
        assert_eq!(
            recovered.deliveries[0].status,
            ContinuityDeliveryStatus::Accepted
        );
        fs::remove_file(root).ok();
    }

    #[test]
    fn failed_acceptance_write_retains_the_submitted_package() {
        let root = std::env::temp_dir().join(format!("boss-receipt-{}", Uuid::new_v4()));
        let path = root.join("continuity.json");
        let session = Uuid::new_v4();
        let mut ledger = ContinuityLedger::default();
        ledger.prepare(
            "handoff",
            session,
            4,
            ContinuityDeliveryKind::SessionHandoff,
            10,
        );
        ledger.mark_submitted("handoff", 11);
        ledger.persist(&path).unwrap();
        // Obstruct the atomic rename after loading the submitted record.
        // The caller receives an error, never an acceptance receipt.
        let retained = root.join("retained.json");
        let failed = ContinuityLedger::update(&path, |ledger| {
            fs::rename(&path, &retained).unwrap();
            fs::create_dir(&path).unwrap();
            ledger.mark_accepted(session, 12)
        });
        assert!(failed.is_err());
        fs::remove_dir(&path).unwrap();
        fs::rename(&retained, &path).unwrap();
        let reopened = ContinuityLedger::load(&path).unwrap();
        assert_eq!(
            reopened.pending_for_session(session).unwrap().status,
            ContinuityDeliveryStatus::Submitted
        );
        assert_eq!(reopened.pending_for_session(session).unwrap().attempts, 1);
        assert!(
            ContinuityLedger::update(&path, |ledger| ledger.mark_accepted(session, 13)).unwrap()
        );
        assert!(
            ContinuityLedger::load(&path)
                .unwrap()
                .pending_for_session(session)
                .is_none()
        );
        fs::remove_dir_all(root).ok();
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

    #[test]
    fn concurrent_delivery_and_incident_updates_survive_restart() {
        let root = std::env::temp_dir().join(format!("boss-ledger-race-{}", Uuid::new_v4()));
        let path = root.join("continuity.json");
        let session = Uuid::new_v4();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
        std::thread::scope(|scope| {
            for index in 0..8 {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    ContinuityLedger::update(path, |ledger| {
                        let id = format!("handoff-{index}");
                        ledger.prepare(&id, session, 1, ContinuityDeliveryKind::SessionHandoff, 10);
                        ledger.mark_submitted(&id, 11);
                    })
                    .unwrap();
                });
            }
            barrier.wait();
            ContinuityLedger::update(&path, |ledger| {
                ledger.record_compaction_incident(session, 12, "controlled incident");
            })
            .unwrap();
        });
        let reopened = ContinuityLedger::load(&path).unwrap();
        assert_eq!(reopened.deliveries.len(), 8);
        assert!(reopened.deliveries.iter().all(|delivery| delivery.status
            == ContinuityDeliveryStatus::Submitted
            && delivery.attempts == 1));
        assert!(reopened.has_incident(session));
        assert!(reopened.rotation_request.is_some());
        fs::remove_dir_all(root).ok();
    }
}
