//! Fail-closed adapter contract. Local attempts are never provider receipts.
use crate::rotation_continuity::{ContinuityAttempt, ContinuityPackage, ContinuityScope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CoordinatorBinding {
    pub provider: waku_protocol::model::ProviderKind,
    pub session: Uuid,
    pub generation: u64,
    pub identity_revision: String,
    pub grants_revision: String,
    pub model: String,
    pub scope: ContinuityScope,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CoordinatorPackage {
    pub binding: CoordinatorBinding,
    pub content: ContinuityPackage,
    /// Enumerable durable references; narrative cannot settle or authorize these.
    pub event_ids: Vec<Uuid>,
    pub effect_ids: Vec<String>,
    pub work_ids: Vec<Uuid>,
}

/// Native identities must come from the provider's transport acknowledgement.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProviderPackageReceipt {
    pub attempt: ContinuityAttempt,
    pub native_session: String,
    pub native_request: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PackageOutcome {
    Confirmed(ProviderPackageReceipt),
    DefinitivelyNotAccepted(ContinuityAttempt),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinatorCapability {
    TypedCoordination,
    ApprovedPlan,
    ApprovedMemory,
    Shell,
    ProjectResearch,
    Web,
    Build,
    Test,
    NativeDelegation,
    Extensions,
    InheritedMcp,
}

pub fn permits(capability: CoordinatorCapability) -> bool {
    matches!(
        capability,
        CoordinatorCapability::TypedCoordination
            | CoordinatorCapability::ApprovedPlan
            | CoordinatorCapability::ApprovedMemory
    )
}

impl CoordinatorPackage {
    pub fn validate_current(&self, current: &CoordinatorBinding) -> anyhow::Result<()> {
        anyhow::ensure!(&self.binding == current, "coordinator binding changed");
        anyhow::ensure!(
            self.content.identity_revision == current.identity_revision
                && self.content.scope == current.scope,
            "instruction binding changed"
        );
        anyhow::ensure!(
            !current.session.is_nil()
                && !current.model.is_empty()
                && !current.grants_revision.is_empty()
                && !current.identity_revision.is_empty(),
            "incomplete coordinator binding"
        );
        Ok(())
    }
}

impl crate::rotation_continuity::ContinuityLedger {
    pub fn prepare_coordinator(
        &mut self,
        id: &str,
        package: &CoordinatorPackage,
        now: u64,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            package.content.identity_revision == package.binding.identity_revision
                && package.content.scope == package.binding.scope,
            "instruction binding changed"
        );
        // The exact digest binds all authority and durable references, not just prose.
        let content = package.exact_content()?;
        self.prepare_package(
            id,
            package.binding.session,
            package.binding.generation,
            content,
            now,
        )
    }

    pub fn record_coordinator_outcome(
        &mut self,
        expected: &ContinuityAttempt,
        outcome: PackageOutcome,
        now: u64,
    ) -> bool {
        match outcome {
            PackageOutcome::Unknown => false,
            PackageOutcome::DefinitivelyNotAccepted(attempt) if &attempt == expected => {
                self.reject_package(&attempt, now)
            }
            PackageOutcome::Confirmed(receipt)
                if &receipt.attempt == expected
                    && !receipt.native_session.is_empty()
                    && !receipt.native_request.is_empty() =>
            {
                if self
                    .provider_receipts
                    .iter()
                    .any(|old| old.attempt == receipt.attempt && old != &receipt)
                {
                    return false;
                }
                if !self.accept_package(&receipt.attempt, now) {
                    return false;
                }
                if !self.provider_receipts.contains(&receipt) {
                    self.provider_receipts.push(receipt);
                }
                true
            }
            _ => false,
        }
    }
}

impl CoordinatorPackage {
    pub fn exact_content(&self) -> anyhow::Result<ContinuityPackage> {
        Ok(ContinuityPackage {
            identity_revision: self.binding.identity_revision.clone(),
            scope: self.binding.scope,
            instructions: self.content.instructions.clone(),
            context: serde_json::to_string(self)?,
        })
    }
}

/// Unknown external effects stay durable and may only be resolved by evidence.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CoordinatorEffect {
    pub id: String,
    pub binding: CoordinatorBinding,
    pub resolution: Option<String>,
}

impl crate::rotation_continuity::ContinuityLedger {
    pub fn begin_coordinator_effect(
        &mut self,
        id: &str,
        binding: &CoordinatorBinding,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!id.is_empty(), "missing effect identity");
        anyhow::ensure!(
            !self
                .coordinator_effects
                .iter()
                .any(|effect| effect.id == id),
            "effect requires reconciliation; replay denied"
        );
        self.coordinator_effects.push(CoordinatorEffect {
            id: id.into(),
            binding: binding.clone(),
            resolution: None,
        });
        Ok(())
    }
    pub fn resolve_coordinator_effect(&mut self, id: &str, evidence: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!evidence.is_empty(), "missing effect evidence");
        let effect = self
            .coordinator_effects
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown effect"))?;
        anyhow::ensure!(
            effect
                .resolution
                .as_deref()
                .is_none_or(|old| old == evidence),
            "effect evidence conflict"
        );
        effect.resolution = Some(evidence.into());
        Ok(())
    }
}

/// Eligibility only; never publishes ownership or activates a successor.
pub fn validate_readiness(
    package: &CoordinatorPackage,
    current_snapshot: &CoordinatorPackage,
    ledger: &crate::rotation_continuity::ContinuityLedger,
    attempt: &ContinuityAttempt,
    expected_budget: &crate::provider_budget::StartupBudgetBinding,
    observed: Option<&crate::provider_budget::StartupBudgetEvidence>,
    policy_enforced: bool,
) -> anyhow::Result<()> {
    let current = &current_snapshot.binding;
    package.validate_current(current)?;
    anyhow::ensure!(
        package == current_snapshot,
        "current continuity/event snapshot changed"
    );
    anyhow::ensure!(
        attempt.session_id == current.session
            && attempt.generation == current.generation
            && attempt.package_digest == package.exact_content()?.digest()?,
        "delivery binding changed"
    );
    anyhow::ensure!(
        ledger
            .provider_receipts
            .iter()
            .any(|r| &r.attempt == attempt
                && !r.native_session.is_empty()
                && !r.native_request.is_empty()),
        "provider acceptance unconfirmed"
    );
    anyhow::ensure!(
        ledger
            .deliveries
            .iter()
            .any(|d| d.attempt.as_ref() == Some(attempt)
                && d.status == crate::rotation_continuity::ContinuityDeliveryStatus::Accepted),
        "delivery not accepted"
    );
    anyhow::ensure!(
        package.effect_ids.iter().all(|id| ledger
            .coordinator_effects
            .iter()
            .any(|effect| &effect.id == id && effect.resolution.is_some())),
        "referenced effect evidence unavailable"
    );
    anyhow::ensure!(
        !ledger
            .coordinator_effects
            .iter()
            .any(|e| e.resolution.is_none()),
        "external effects require reconciliation"
    );
    anyhow::ensure!(
        expected_budget.runtime_id == attempt.runtime_id
            && expected_budget.generation == current.generation
            && expected_budget.provider == current.provider
            && expected_budget.model == current.model
            && expected_budget.instruction_revision == current.identity_revision
            && expected_budget.package_digest == attempt.package_digest,
        "startup binding changed"
    );
    anyhow::ensure!(policy_enforced, "coordinator policy unsupported");
    crate::provider_budget::validate_startup_budget(expected_budget, observed)
        .map_err(|blocker| anyhow::anyhow!("startup blocked: {blocker:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rotation_continuity::{ContinuityDeliveryStatus, ContinuityLedger};
    fn package() -> CoordinatorPackage {
        CoordinatorPackage {
            binding: CoordinatorBinding {
                provider: waku_protocol::model::ProviderKind::Codex,
                session: Uuid::new_v4(),
                generation: 3,
                identity_revision: "canonical-7".into(),
                grants_revision: "grants-4".into(),
                model: "model".into(),
                scope: ContinuityScope::Planning,
            },
            content: ContinuityPackage {
                identity_revision: "canonical-7".into(),
                scope: ContinuityScope::Planning,
                instructions: "Designer only; no implementation or finalization".into(),
                context: "unmodified pending content".into(),
            },
            event_ids: vec![Uuid::new_v4()],
            effect_ids: vec!["ambiguous-effect".into()],
            work_ids: vec![Uuid::new_v4()],
        }
    }
    #[test]
    fn coordinator_contract_preserves_exact_package_unknown_attempt_and_effects_after_restart() {
        let path = std::env::temp_dir().join(format!("coordinator-{}.json", Uuid::new_v4()));
        let package = package();
        let attempt = ContinuityLedger::update(&path, |ledger| -> anyhow::Result<_> {
            ledger.prepare_coordinator("delivery", &package, 1)?;
            ledger.begin_coordinator_effect("ambiguous-effect", &package.binding)?;
            ledger.submit_package("delivery", Uuid::new_v4(), 2)
        })
        .unwrap()
        .unwrap();
        let mut reopened = ContinuityLedger::load(&path).unwrap();
        assert_eq!(
            reopened.deliveries[0].package,
            Some(package.exact_content().unwrap())
        );
        assert!(!reopened.record_coordinator_outcome(&attempt, PackageOutcome::Unknown, 3));
        assert!(
            reopened
                .submit_package("delivery", attempt.runtime_id, 4)
                .is_err()
        );
        assert!(
            reopened
                .begin_coordinator_effect("ambiguous-effect", &package.binding)
                .is_err()
        );
        assert_eq!(reopened.coordinator_effects[0].resolution, None);
        let mut receipt = ProviderPackageReceipt {
            attempt: attempt.clone(),
            native_session: String::new(),
            native_request: "native-request".into(),
        };
        assert!(!reopened.record_coordinator_outcome(
            &attempt,
            PackageOutcome::Confirmed(receipt.clone()),
            4
        ));
        receipt.native_session = "native-session".into();
        let mut foreign = receipt.clone();
        foreign.attempt.generation += 1;
        assert!(!reopened.record_coordinator_outcome(
            &attempt,
            PackageOutcome::Confirmed(foreign),
            4
        ));
        assert!(reopened.record_coordinator_outcome(
            &attempt,
            PackageOutcome::Confirmed(receipt.clone()),
            4
        ));
        assert!(reopened.record_coordinator_outcome(
            &attempt,
            PackageOutcome::Confirmed(receipt.clone()),
            4
        ));
        receipt.native_request = "different-native-request".into();
        assert!(!reopened.record_coordinator_outcome(
            &attempt,
            PackageOutcome::Confirmed(receipt),
            4
        ));
        assert_eq!(reopened.provider_receipts.len(), 1);
        assert_eq!(
            reopened.deliveries[0].status,
            ContinuityDeliveryStatus::Accepted
        );
        assert!(
            reopened
                .resolve_coordinator_effect("ambiguous-effect", "")
                .is_err()
        );
        reopened
            .resolve_coordinator_effect("ambiguous-effect", "external reconciliation record")
            .unwrap();
        assert!(
            reopened
                .begin_coordinator_effect("ambiguous-effect", &package.binding)
                .is_err()
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn coordinator_contract_rejects_changed_authority_and_denies_all_execution_surfaces() {
        let package = package();
        for field in 0..6 {
            let mut changed = package.binding.clone();
            match field {
                0 => changed.session = Uuid::new_v4(),
                1 => changed.generation += 1,
                2 => changed.identity_revision.push('x'),
                3 => changed.grants_revision.push('x'),
                4 => changed.model.push('x'),
                _ => changed.scope = ContinuityScope::Coordinator,
            }
            assert!(package.validate_current(&changed).is_err());
        }
        use CoordinatorCapability::*;
        for capability in [
            Shell,
            ProjectResearch,
            Web,
            Build,
            Test,
            NativeDelegation,
            Extensions,
            InheritedMcp,
        ] {
            assert!(!permits(capability));
        }
        for capability in [TypedCoordination, ApprovedPlan, ApprovedMemory] {
            assert!(permits(capability));
        }
    }
    #[test]
    fn coordinator_contract_definitive_rejection_only_allows_correlated_retry() {
        let package = package();
        let mut ledger = ContinuityLedger::default();
        ledger.prepare_coordinator("delivery", &package, 1).unwrap();
        let first = ledger
            .submit_package("delivery", Uuid::new_v4(), 2)
            .unwrap();
        let mut foreign = first.clone();
        foreign.attempt_id = Uuid::new_v4();
        assert!(!ledger.record_coordinator_outcome(
            &first,
            PackageOutcome::DefinitivelyNotAccepted(foreign),
            3
        ));
        assert!(ledger.record_coordinator_outcome(
            &first,
            PackageOutcome::DefinitivelyNotAccepted(first.clone()),
            3
        ));
        let second = ledger
            .submit_package("delivery", first.runtime_id, 4)
            .unwrap();
        assert_ne!(first.attempt_id, second.attempt_id);
        assert!(!ledger.record_coordinator_outcome(
            &second,
            PackageOutcome::DefinitivelyNotAccepted(first),
            5
        ));
    }
}

#[cfg(test)]
mod readiness_tests {
    use super::*;
    use crate::{provider_budget::*, rotation_continuity::*};
    #[test]
    fn coordinator_readiness_rejects_legacy_receipts_stale_events_and_safe_arithmetic() {
        let binding = CoordinatorBinding {
            provider: waku_protocol::model::ProviderKind::Codex,
            session: Uuid::new_v4(),
            generation: 2,
            identity_revision: "canonical".into(),
            grants_revision: "grants".into(),
            model: "model".into(),
            scope: ContinuityScope::Coordinator,
        };
        let package = CoordinatorPackage {
            content: ContinuityPackage {
                identity_revision: binding.identity_revision.clone(),
                scope: binding.scope,
                instructions: "canonical instructions".into(),
                context: "pending context".into(),
            },
            binding,
            event_ids: vec![Uuid::new_v4()],
            work_ids: vec![Uuid::new_v4()],
            effect_ids: vec![],
        };
        let mut ledger = ContinuityLedger::default();
        ledger.prepare_coordinator("delivery", &package, 1).unwrap();
        let attempt = ledger
            .submit_package("delivery", Uuid::new_v4(), 2)
            .unwrap();
        let expected = StartupBudgetBinding {
            provider: package.binding.provider,
            runtime_version: "synthetic-effective".into(),
            model: package.binding.model.clone(),
            configuration_revision: "config".into(),
            instruction_revision: package.binding.identity_revision.clone(),
            runtime_id: attempt.runtime_id,
            generation: package.binding.generation,
            package_digest: package.exact_content().unwrap().digest().unwrap(),
            operation_id: Uuid::new_v4(),
        };
        let observed = StartupBudgetEvidence {
            binding: expected.clone(),
            bounds: BudgetBounds {
                compaction_floor: Some(1000),
                current_context: Some(1),
                max_growth: Some(1),
                safety_margin: Some(1),
            },
        };
        // A local correlation accepted by the compatibility ledger is not a native receipt.
        assert!(ledger.accept_package(&attempt, 3));
        assert!(
            validate_readiness(
                &package,
                &package,
                &ledger,
                &attempt,
                &expected,
                Some(&observed),
                true
            )
            .unwrap_err()
            .to_string()
            .contains("unconfirmed")
        );
        assert!(ledger.record_coordinator_outcome(
            &attempt,
            PackageOutcome::Confirmed(ProviderPackageReceipt {
                attempt: attempt.clone(),
                native_session: "native-session".into(),
                native_request: "native-request".into()
            }),
            4
        ));
        let mut changed = package.clone();
        changed.event_ids.push(Uuid::new_v4());
        assert!(
            validate_readiness(
                &package,
                &changed,
                &ledger,
                &attempt,
                &expected,
                Some(&observed),
                true
            )
            .unwrap_err()
            .to_string()
            .contains("snapshot changed")
        );
        assert!(
            validate_readiness(
                &package,
                &package,
                &ledger,
                &attempt,
                &expected,
                Some(&observed),
                true
            )
            .unwrap_err()
            .to_string()
            .contains("UnqualifiedRuntime")
        );
        // Legacy-history pruning must never remove exact authority/receipt evidence.
        for i in 0..40 {
            let session = Uuid::new_v4();
            let id = format!("legacy-{i}");
            ledger.prepare(&id, session, 1, ContinuityDeliveryKind::SessionHandoff, 5);
            ledger.mark_submitted(&id, 6);
            ledger.mark_accepted(session, 7);
        }
        assert!(
            ledger
                .deliveries
                .iter()
                .any(|d| d.attempt.as_ref() == Some(&attempt))
        );
        assert_eq!(ledger.provider_receipts.len(), 1);
    }
}
