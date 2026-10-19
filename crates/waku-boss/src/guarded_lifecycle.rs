//! Durable, fail-closed checkpoints for coordinator replacement.
//!
//! A prepared successor has no authority. Native receipt, current continuity,
//! policy and independently observed budget must all validate before publication.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::coordinator_contract::{CoordinatorPackage, validate_readiness};
use crate::provider_budget::{
    StartupBudgetBinding, StartupBudgetEvidence, validate_startup_budget,
};
use crate::rotation_continuity::{ContinuityAttempt, ContinuityLedger};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FrozenCoordinator {
    pub predecessor: Uuid,
    pub successor: Uuid,
    pub generation: u64,
    /// Immutable checkpoint; new intake stays queued outside this package.
    pub package: Option<CoordinatorPackage>,
}

impl ContinuityLedger {
    /// Freeze before waiting for quiescence. Duplicate triggers keep the original
    /// successor and checkpoint. A different replacement cannot discard evidence.
    pub fn freeze_coordinator(
        &mut self,
        predecessor: Uuid,
        successor: Uuid,
        generation: u64,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !predecessor.is_nil() && !successor.is_nil() && predecessor != successor,
            "invalid coordinator replacement"
        );
        generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("generation exhausted"))?;
        if let Some(frozen) = &self.frozen_coordinator {
            anyhow::ensure!(
                frozen.predecessor == predecessor
                    && frozen.successor == successor
                    && frozen.generation == generation,
                "another coordinator replacement remains unresolved"
            );
        } else {
            self.frozen_coordinator = Some(FrozenCoordinator {
                predecessor,
                successor,
                generation,
                package: None,
            });
        }
        Ok(())
    }

    pub fn coordinator_frozen(&self, session: Uuid) -> bool {
        self.frozen_coordinator
            .as_ref()
            .is_some_and(|f| f.predecessor == session || f.successor == session)
    }

    /// Staging does not submit, consume queued events, or grant ownership.
    pub fn stage_successor(
        &mut self,
        id: &str,
        package: &CoordinatorPackage,
        now: u64,
    ) -> anyhow::Result<()> {
        let frozen = self
            .frozen_coordinator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("coordinator was not frozen"))?;
        anyhow::ensure!(
            package.binding.session == frozen.successor
                && Some(package.binding.generation) == frozen.generation.checked_add(1),
            "successor generation changed"
        );
        if let Some(staged) = &frozen.package {
            anyhow::ensure!(staged == package, "frozen continuity checkpoint changed");
        }
        self.prepare_coordinator(id, package, now)?;
        self.frozen_coordinator
            .as_mut()
            .expect("checked freeze")
            .package = Some(package.clone());
        Ok(())
    }

    /// Called under the coordinator operation lock immediately before pointer
    /// publication, including recovery of a pointer published by older code.
    /// The caller must supply a fresh authoritative snapshot, never the saved
    /// package as a substitute for current state. No current provider passes.
    pub fn validate_successor(
        &self,
        current: &CoordinatorPackage,
        attempt: &ContinuityAttempt,
        expected: &StartupBudgetBinding,
        evidence: Option<&StartupBudgetEvidence>,
        policy: bool,
    ) -> anyhow::Result<()> {
        let frozen = self
            .frozen_coordinator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("coordinator was not frozen"))?;
        let package = frozen
            .package
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("successor checkpoint unavailable"))?;
        anyhow::ensure!(
            current.binding.session == frozen.successor,
            "wrong successor"
        );
        validate_readiness(package, current, self, attempt, expected, evidence, policy)
    }
}

/// Validate each request, continuation, resume, replay or autonomous checkpoint.
/// There is no cached arithmetic permission. The binding must name this operation
/// and the actual runtime; absent adapter observations always block. Frozen old
/// and preparing new generations cannot grow even with fresh safe arithmetic.
pub fn validate_growth(
    ledger: &ContinuityLedger,
    session: Uuid,
    generation: u64,
    expected: &StartupBudgetBinding,
    evidence: Option<&StartupBudgetEvidence>,
) -> anyhow::Result<u64> {
    anyhow::ensure!(
        !ledger.coordinator_frozen(session),
        "coordinator refresh is frozen"
    );
    anyhow::ensure!(
        expected.generation == generation,
        "stale coordinator generation"
    );
    validate_startup_budget(expected, evidence)
        .map_err(|cause| anyhow::anyhow!("coordinator growth blocked: {cause:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator_contract::{CoordinatorBinding, PackageOutcome};
    use crate::provider_budget::{BudgetBounds, validate_context_budget};
    use crate::rotation_continuity::{ContinuityPackage, ContinuityScope};
    use waku_protocol::model::ProviderKind;

    fn package(session: Uuid) -> CoordinatorPackage {
        CoordinatorPackage {
            binding: CoordinatorBinding {
                provider: ProviderKind::Codex,
                session,
                generation: 1,
                identity_revision: "identity".into(),
                grants_revision: "grants".into(),
                model: "model".into(),
                scope: ContinuityScope::Coordinator,
            },
            content: ContinuityPackage {
                identity_revision: "identity".into(),
                scope: ContinuityScope::Coordinator,
                instructions: "canonical coordinator restrictions".into(),
                context: "unfinished intent".into(),
            },
            event_ids: vec![Uuid::new_v4()],
            work_ids: vec![Uuid::new_v4()],
            effect_ids: vec![],
        }
    }

    fn binding(package: &CoordinatorPackage) -> StartupBudgetBinding {
        StartupBudgetBinding {
            provider: package.binding.provider,
            runtime_version: "fixture".into(),
            model: package.binding.model.clone(),
            configuration_revision: "configuration".into(),
            instruction_revision: package.binding.identity_revision.clone(),
            runtime_id: Uuid::new_v4(),
            generation: package.binding.generation,
            package_digest: package.exact_content().unwrap().digest().unwrap(),
            operation_id: Uuid::new_v4(),
        }
    }

    #[test]
    fn guarded_lifecycle_restart_and_duplicate_triggers_preserve_checkpoint() {
        let path = std::env::temp_dir().join(format!("guarded-lifecycle-{}.json", Uuid::new_v4()));
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        let package = package(new);
        for stage in 0..4 {
            let mut ledger = ContinuityLedger::load(&path).unwrap();
            ledger.freeze_coordinator(old, new, 0).unwrap();
            assert!(ledger.freeze_coordinator(old, Uuid::new_v4(), 0).is_err());
            if stage > 0 {
                ledger.stage_successor("delivery", &package, 1).unwrap();
            }
            if stage == 2 {
                ledger
                    .submit_package("delivery", Uuid::new_v4(), 2)
                    .unwrap();
            }
            if stage == 3 {
                let attempt = ledger.deliveries[0].attempt.clone().unwrap();
                assert!(!ledger.record_coordinator_outcome(&attempt, PackageOutcome::Unknown, 3));
                assert!(
                    ledger
                        .submit_package("delivery", Uuid::new_v4(), 3)
                        .is_err()
                );
            }
            ledger.persist(&path).unwrap();
            let restored = ContinuityLedger::load(&path).unwrap();
            assert_eq!(restored, ledger);
            assert!(restored.coordinator_frozen(old));
            assert!(restored.coordinator_frozen(new));
            assert_eq!(restored.frozen_coordinator.unwrap().successor, new);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn guarded_lifecycle_stale_identity_events_and_effects_cannot_replace_checkpoint() {
        let mut ledger = ContinuityLedger::default();
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        let original = package(new);
        ledger.freeze_coordinator(old, new, 0).unwrap();
        ledger.stage_successor("delivery", &original, 0).unwrap();
        for field in 0..7 {
            let mut changed = original.clone();
            match field {
                0 => changed.binding.generation += 1,
                1 => changed.binding.identity_revision.push('x'),
                2 => changed.binding.grants_revision.push('x'),
                3 => changed.binding.model.push('x'),
                4 => changed.event_ids.push(Uuid::new_v4()),
                5 => changed.effect_ids.push("uncertain effect".into()),
                _ => changed.content.context.push('x'),
            }
            let before = ledger.clone();
            assert!(ledger.stage_successor("delivery", &changed, 1).is_err());
            assert_eq!(before, ledger);
        }
    }

    #[test]
    fn guarded_lifecycle_every_growth_boundary_requires_fresh_evidence() {
        let mut ledger = ContinuityLedger::default();
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        let package = package(new);
        let expected = binding(&package);
        for _boundary in [
            "startup",
            "request",
            "tool-result",
            "continuation",
            "resume",
            "replay",
            "autonomous",
        ] {
            assert!(validate_growth(&ledger, new, 1, &expected, None).is_err());
            let mut stale = expected.clone();
            stale.operation_id = Uuid::new_v4();
            let evidence = StartupBudgetEvidence {
                binding: stale,
                bounds: BudgetBounds {
                    compaction_floor: Some(100),
                    current_context: Some(0),
                    max_growth: Some(1),
                    safety_margin: Some(1),
                },
            };
            assert!(validate_growth(&ledger, new, 1, &expected, Some(&evidence)).is_err());
            assert!(validate_growth(&ledger, new, 0, &expected, Some(&evidence)).is_err());
        }
        ledger.freeze_coordinator(old, new, 0).unwrap();
        let evidence = StartupBudgetEvidence {
            binding: expected.clone(),
            bounds: BudgetBounds {
                compaction_floor: Some(100),
                current_context: Some(0),
                max_growth: Some(1),
                safety_margin: Some(1),
            },
        };
        assert!(
            validate_growth(&ledger, new, 1, &expected, Some(&evidence))
                .unwrap_err()
                .to_string()
                .contains("frozen")
        );
        assert!(ledger.freeze_coordinator(old, new, u64::MAX).is_err());
    }

    #[test]
    fn guarded_lifecycle_long_loops_and_oversized_results_stop_at_equality() {
        let mut bounds = BudgetBounds {
            compaction_floor: Some(100),
            current_context: Some(0),
            max_growth: Some(10),
            safety_margin: Some(1),
        };
        // This is a synthetic arithmetic oracle, never runtime qualification.
        let mut admitted = 0;
        for current in 0..100 {
            bounds.current_context = Some(current);
            if validate_context_budget(&bounds).is_ok() {
                admitted += 1;
            }
        }
        assert_eq!(admitted, 89);
        for oversized in [89, 90, 100, u64::MAX] {
            bounds.current_context = Some(oversized);
            assert!(validate_context_budget(&bounds).is_err());
        }
        bounds.current_context = None;
        assert!(validate_context_budget(&bounds).is_err());
        bounds.current_context = Some(0);
        bounds.max_growth = None;
        assert!(validate_context_budget(&bounds).is_err());
    }

    #[test]
    fn guarded_lifecycle_native_receipt_cannot_activate_unqualified_or_ambiguous_effect() {
        use crate::coordinator_contract::ProviderPackageReceipt;
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        let package = package(new);
        let mut ledger = ContinuityLedger::default();
        ledger.freeze_coordinator(old, new, 0).unwrap();
        ledger.stage_successor("delivery", &package, 0).unwrap();
        let attempt = ledger
            .submit_package("delivery", Uuid::new_v4(), 1)
            .unwrap();
        let mut expected = binding(&package);
        expected.runtime_id = attempt.runtime_id;
        assert!(
            ledger
                .validate_successor(&package, &attempt, &expected, None, true)
                .is_err()
        );
        assert!(ledger.record_coordinator_outcome(
            &attempt,
            PackageOutcome::Confirmed(ProviderPackageReceipt {
                attempt: attempt.clone(),
                native_session: "fixture-native-session".into(),
                native_request: "fixture-native-request".into(),
            }),
            2,
        ));
        let evidence = StartupBudgetEvidence {
            binding: expected.clone(),
            bounds: BudgetBounds {
                compaction_floor: Some(100),
                current_context: Some(0),
                max_growth: Some(1),
                safety_margin: Some(1),
            },
        };
        assert!(
            ledger
                .validate_successor(&package, &attempt, &expected, Some(&evidence), true)
                .unwrap_err()
                .to_string()
                .contains("UnqualifiedRuntime")
        );
        ledger
            .begin_coordinator_effect("unknown", &package.binding)
            .unwrap();
        assert!(
            ledger
                .validate_successor(&package, &attempt, &expected, Some(&evidence), true)
                .unwrap_err()
                .to_string()
                .contains("reconciliation")
        );
        assert!(
            ledger
                .begin_coordinator_effect("unknown", &package.binding)
                .is_err()
        );
    }
}
